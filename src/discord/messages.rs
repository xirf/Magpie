use super::state::{get_resolved_user, translate, Handler};
use crate::utils::{
    convert_eta, convert_size, extract_clean_magnet, extract_hash_from_magnet, format_progress,
};
use serenity::builder::{CreateActionRow, CreateButton};
use serenity::model::application::ButtonStyle;
use serenity::model::channel::Message;
use serenity::prelude::*;

pub async fn handle_message(handler: &Handler, ctx: &Context, msg: Message) {
    if msg.author.bot {
        return;
    }

    let is_dm = msg.guild_id.is_none();
    let bot_id = match ctx.http.get_current_user().await {
        Ok(u) => u.id,
        Err(_) => return,
    };
    let is_mentioned = msg.mentions_user_id(bot_id);

    if !is_dm && !is_mentioned {
        return;
    }

    let settings = handler.settings.read().await.clone();
    let mut user = get_resolved_user(&settings, &msg.author.id.to_string());

    // Check if there is any administrator
    let has_admin = settings.users.iter().any(|u| {
        u.role == "administrator" && u.discord_id.as_deref().unwrap_or("") != "9876543210123"
    });
    if !has_admin {
        user.role = "administrator".to_string();
        user.discord_id = Some(msg.author.id.to_string());
        {
            let mut settings_write = handler.settings.write().await;
            settings_write.users.push(user.clone());
            settings_write.export_settings();
            let _ = crate::db::save_user_to_db(&user);
        }
        let _ = msg
            .reply(
                &ctx.http,
                "👑 You have been automatically authorized as the first **administrator**!",
            )
            .await;
    } else if settings
        .users
        .iter()
        .all(|u| u.discord_id.as_deref() != Some(&msg.author.id.to_string()))
    {
        let row = CreateActionRow::Buttons(vec![CreateButton::new("dc_request_access")
            .label("Request Access")
            .style(ButtonStyle::Primary)]);
        let _ = msg
            .channel_id
            .send_message(
                &ctx.http,
                serenity::builder::CreateMessage::new()
                    .content("❌ You are not authorized to use this bot.")
                    .components(vec![row])
                    .reference_message(&msg),
            )
            .await;
        return;
    }

    if user.role == "reader" {
        let _ = msg
            .reply(
                &ctx.http,
                translate(&user, "You are not authorized to use this bot", None),
            )
            .await;
        return;
    }

    // 0. Check if this is a reply referencing a message
    if let Some(ref ref_msg) = msg.referenced_message {
        let text = msg.content.trim().to_lowercase();
        if text == "info" || text == "/info" || text == "link" || text == "/link" {
            let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
            match crate::db::get_torrent_hash_for_message(&ref_msg.id.to_string()) {
                Ok(Some(hash)) => {
                    if text.contains("info") {
                        match handler.manager.get_torrent(&hash, None).await {
                            Ok(Some(t)) => {
                                let progress_percent = (t.progress * 100.0).round() as i32;
                                let embed = serenity::builder::CreateEmbed::new()
                                    .title(&t.name)
                                    .color(0x00ae86)
                                    .field(
                                        "Progress",
                                        format!(
                                            "{} {}%",
                                            format_progress(t.progress, 20),
                                            progress_percent
                                        ),
                                        false,
                                    )
                                    .field("State", format!("`{}`", t.state), true)
                                    .field("Size", format!("`{}`", convert_size(t.size)), true)
                                    .field(
                                        "Download Speed",
                                        format!("`{}/s`", convert_size(t.dlspeed)),
                                        true,
                                    )
                                    .field("ETA", format!("`{}`", convert_eta(t.eta)), true)
                                    .field("Hash", format!("`{}`", t.hash), false);
                                let _ = msg
                                    .channel_id
                                    .send_message(
                                        &ctx.http,
                                        serenity::builder::CreateMessage::new()
                                            .embed(embed)
                                            .reference_message(&msg),
                                    )
                                    .await;
                            }
                            _ => {
                                let _ = msg
                                    .reply(&ctx.http, "❌ Torrent not found in client.")
                                    .await;
                            }
                        }
                    } else {
                        match handler.manager.get_torrent(&hash, None).await {
                            Ok(Some(t)) => {
                                if t.progress >= 1.0 {
                                    match crate::s3::get_download_link(&settings, &t).await {
                                        Ok(Some(url)) => {
                                            let row = CreateActionRow::Buttons(vec![
                                                CreateButton::new_link(url).label("Download"),
                                            ]);
                                            let content = if settings.local_server.enabled {
                                                format!(
                                                    "🔗 Here is your download link for **{}**:",
                                                    t.name
                                                )
                                            } else {
                                                format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", t.name)
                                            };
                                            let _ = msg
                                                .channel_id
                                                .send_message(
                                                    &ctx.http,
                                                    serenity::builder::CreateMessage::new()
                                                        .content(content)
                                                        .components(vec![row])
                                                        .reference_message(&msg),
                                                )
                                                .await;
                                        }
                                        Ok(None) => {
                                            let _ = msg.reply(&ctx.http, "❌ Download links are either disabled or not configured.").await;
                                        }
                                        Err(e) => {
                                            let _ = msg
                                                .reply(&ctx.http, format!("❌ Error: {}", e))
                                                .await;
                                        }
                                    }
                                } else {
                                    let _ = msg
                                        .reply(&ctx.http, "❌ Torrent is not fully completed yet.")
                                        .await;
                                }
                            }
                            _ => {
                                let _ = msg
                                    .reply(&ctx.http, "❌ Torrent not found in client.")
                                    .await;
                            }
                        }
                    }
                }
                Ok(None) => {
                    let _ = msg
                        .reply(
                            &ctx.http,
                            "❌ This message is not associated with any torrent.",
                        )
                        .await;
                }
                Err(e) => {
                    let _ = msg
                        .reply(&ctx.http, format!("❌ Database error: {}", e))
                        .await;
                }
            }
            return;
        }
    }

    // 1. Check for magnet link
    if let Some(magnet_link) = extract_clean_magnet(&msg.content) {
        let retry_row = CreateActionRow::Buttons(vec![CreateButton::new("dc_retry")
            .label("Retry")
            .style(ButtonStyle::Primary)]);

        let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

        match handler.manager.add_magnet(&magnet_link, None).await {
            Ok(true) => {
                let hash_opt = extract_hash_from_magnet(&magnet_link);
                let mut details_msg = "✅ **Magnet link added successfully!**".to_string();
                if let Some(ref h) = hash_opt {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    if let Ok(Some(t)) = handler.manager.get_torrent(h, None).await {
                        let peers_str = match (t.num_seeds, t.num_peers) {
                            (Some(seeds), Some(peers)) => {
                                format!("Seeds: `{}` | Peers: `{}`", seeds, peers)
                            }
                            (None, Some(peers)) => format!("Peers: `{}`", peers),
                            (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                            (None, None) => "Unknown".to_string(),
                        };
                        details_msg = format!("✅ **Magnet link added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash);
                    }
                }
                if let Ok(mut sent_msg) = msg.reply(&ctx.http, details_msg).await {
                    if let Some(h) = hash_opt {
                        let _ = crate::db::associate_message_with_torrent(
                            &sent_msg.id.to_string(),
                            &h,
                            None,
                        );
                        let manager = handler.manager.clone();
                        let http = ctx.http.clone();
                        tokio::spawn(async move {
                            for _ in 0..15 {
                                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                if let Ok(Some(t)) = manager.get_torrent(&h, None).await {
                                    if t.size > 0 && t.name != h && !t.name.is_empty() {
                                        let peers_str = match (t.num_seeds, t.num_peers) {
                                            (Some(seeds), Some(peers)) => {
                                                format!("Seeds: `{}` | Peers: `{}`", seeds, peers)
                                            }
                                            (None, Some(peers)) => format!("Peers: `{}`", peers),
                                            (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                            (None, None) => "Unknown".to_string(),
                                        };
                                        let updated_msg = format!("✅ **Magnet link added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash);
                                        let _ = sent_msg
                                            .edit(
                                                &http,
                                                serenity::builder::EditMessage::new()
                                                    .content(updated_msg),
                                            )
                                            .await;
                                        break;
                                    }
                                }
                            }
                        });
                    }
                }
            }
            Ok(false) => {
                let _ = msg
                    .channel_id
                    .send_message(
                        &ctx.http,
                        serenity::builder::CreateMessage::new()
                            .content("❌ Failed to add magnet link.")
                            .components(vec![retry_row])
                            .reference_message(&msg),
                    )
                    .await;
            }
            Err(e) => {
                let content = if e.contains("409") {
                    "⚠️ This torrent/magnet link is already in the download list.".to_string()
                } else {
                    format!("❌ Error: {}", e)
                };
                let _ = msg
                    .channel_id
                    .send_message(
                        &ctx.http,
                        serenity::builder::CreateMessage::new()
                            .content(content)
                            .components(vec![retry_row])
                            .reference_message(&msg),
                    )
                    .await;
            }
        }
        return;
    }

    // 2. Check for torrent attachments
    for att in &msg.attachments {
        if att.filename.ends_with(".torrent") {
            let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

            let retry_row = CreateActionRow::Buttons(vec![CreateButton::new("dc_retry")
                .label("Retry")
                .style(ButtonStyle::Primary)]);

            let before = handler
                .manager
                .get_torrents(None, None)
                .await
                .unwrap_or_default();

            match reqwest::get(&att.url).await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => {
                        match handler
                            .manager
                            .add_torrent(bytes.to_vec(), &att.filename, None)
                            .await
                        {
                            Ok(true) => {
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                let after = handler
                                    .manager
                                    .get_torrents(None, None)
                                    .await
                                    .unwrap_or_default();
                                let new_torrent = after.iter().find(|t_after| {
                                    !before.iter().any(|t_before| t_before.hash == t_after.hash)
                                });
                                let (details_msg, hash_opt) = if let Some(t) = new_torrent {
                                    let peers_str = match (t.num_seeds, t.num_peers) {
                                        (Some(seeds), Some(peers)) => {
                                            format!("Seeds: `{}` | Peers: `{}`", seeds, peers)
                                        }
                                        (None, Some(peers)) => format!("Peers: `{}`", peers),
                                        (Some(seeds), None) => format!("Seeds: `{}`", seeds),
                                        (None, None) => "Unknown".to_string(),
                                    };
                                    (format!("✅ **Torrent file added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_str, t.hash), Some(t.hash.clone()))
                                } else {
                                    ("✅ Torrent file added successfully!".to_string(), None)
                                };
                                if let Ok(sent_msg) = msg.reply(&ctx.http, details_msg).await {
                                    if let Some(h) = hash_opt {
                                        let _ = crate::db::associate_message_with_torrent(
                                            &sent_msg.id.to_string(),
                                            &h,
                                            None,
                                        );
                                    }
                                }
                            }
                            Ok(false) => {
                                let _ = msg
                                    .channel_id
                                    .send_message(
                                        &ctx.http,
                                        serenity::builder::CreateMessage::new()
                                            .content("❌ Failed to add torrent file.")
                                            .components(vec![retry_row])
                                            .reference_message(&msg),
                                    )
                                    .await;
                            }
                            Err(e) => {
                                let content = if e.contains("409") {
                                    "⚠️ This torrent/magnet link is already in the download list."
                                        .to_string()
                                } else {
                                    format!("❌ Error: {}", e)
                                };
                                let _ = msg
                                    .channel_id
                                    .send_message(
                                        &ctx.http,
                                        serenity::builder::CreateMessage::new()
                                            .content(content)
                                            .components(vec![retry_row])
                                            .reference_message(&msg),
                                    )
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = msg
                            .reply(&ctx.http, format!("Failed to read attachment: {}", e))
                            .await;
                    }
                },
                Err(e) => {
                    let _ = msg
                        .reply(&ctx.http, format!("Failed to download attachment: {}", e))
                        .await;
                }
            }
            return;
        }
    }
}

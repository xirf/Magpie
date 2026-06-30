use serenity::all::UserId;
use serenity::builder::{CreateActionRow, CreateButton, CreateMessage};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use teloxide::prelude::*;
use teloxide::Bot;
use tokio::sync::RwLock;

use crate::config::{Settings, UserSettings};
use crate::db::{get_messages_for_torrent, is_notification_sent, mark_notification_sent};
use crate::i18n::t;
use crate::redis_client::RedisWrapper;
use crate::torrent_client::TorrentClient;
use crate::utils::{convert_size, escape_markdown, format_progress};

fn user_filters(users: &[UserSettings], category: Option<&str>) -> Vec<UserSettings> {
    users
        .iter()
        .filter(|user| {
            if user.notification_filter.is_empty() {
                return true;
            }
            if let Some(cat) = category {
                user.notification_filter.iter().any(|f| f == cat)
            } else {
                false
            }
        })
        .cloned()
        .collect()
}

pub async fn torrent_finished(
    tg_bot: Option<&Bot>,
    discord_http: Option<Arc<serenity::http::Http>>,
    redis: &RedisWrapper,
    settings: &Settings,
    manager: &dyn TorrentClient,
) {
    let completed_torrents = match manager.get_torrents(None, Some("completed")).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error retrieving completed torrents: {:?}", e);
            return;
        }
    };

    for torrent in completed_torrents {
        let exists_in_redis = redis.exists(&torrent.hash).await;
        let exists_in_sqlite = is_notification_sent(&torrent.hash);

        if !exists_in_redis && !exists_in_sqlite {
            let mut download_link = String::new();

            let has_local_server = settings.local_server.enabled;
            let has_s3 = settings.s3.enabled;

            if has_local_server || has_s3 {
                println!("[Link] Processing completed torrent: {}", torrent.name);

                // If local server is not enabled, but S3 is, and mode is "upload", do the upload
                if !has_local_server && has_s3 && settings.s3.mode == "upload" {
                    let content_path_str = torrent.content_path.clone().unwrap_or_default();
                    if !content_path_str.is_empty() {
                        let content_path = std::path::Path::new(&content_path_str);
                        println!("[S3] Uploading {:?} to bucket...", content_path);
                        if let Err(e) =
                            crate::s3::upload_folder_or_file_to_s3(settings, content_path, "").await
                        {
                            eprintln!("[S3] Failed to upload to S3: {:?}", e);
                            continue;
                        }
                        println!("[S3] Upload completed successfully for {}", torrent.name);
                    } else {
                        eprintln!("[S3] Torrent content path is missing.");
                        continue;
                    }
                }

                match crate::s3::get_download_link(settings, &torrent).await {
                    Ok(Some(url)) => {
                        download_link = url;
                        println!(
                            "[Link] Generated download link for {}: {}",
                            torrent.name, download_link
                        );

                        // If we uploaded to S3, delete local torrent data
                        if !has_local_server && has_s3 && settings.s3.mode == "upload" {
                            if let Err(e) = manager.delete_one_data(&torrent.hash).await {
                                eprintln!("[S3] Failed to delete local torrent data: {:?}", e);
                            } else {
                                println!(
                                    "[S3] Deleted local torrent and data for {}",
                                    torrent.name
                                );
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("[Link] Failed to generate download link: {:?}", e);
                        continue;
                    }
                }
            }

            let target_users = user_filters(&settings.users, torrent.category.as_deref());
            for user in &target_users {
                if user.notify {
                    let user_lang = user.locale.as_deref().unwrap_or("en");

                    let mut vars = HashMap::new();
                    vars.insert("name".to_string(), escape_markdown(&torrent.name));
                    let mut message = t(
                        "Torrent {name} has finished downloading!",
                        user_lang,
                        Some(&vars),
                    );

                    if !download_link.is_empty() && user.discord_id.is_none() {
                        if settings.local_server.enabled {
                            message.push_str(&format!(
                                "\n\n🔗 **[Download Link]({})**",
                                download_link
                            ));
                        } else {
                            message.push_str(&format!(
                                "\n\n🔗 **[Download Link]({})** *(Expires in 1 hour)*",
                                download_link
                            ));
                        }
                    }

                    // Telegram notification
                    if user.user_id != 0 {
                        if let Some(bot) = tg_bot {
                            if let Ok(sent_msg) = bot
                                .send_message(teloxide::types::ChatId(user.user_id), &message)
                                .await
                            {
                                let _ = crate::db::associate_message_with_torrent(
                                    &sent_msg.id.to_string(),
                                    &torrent.hash,
                                    None,
                                );
                            }
                        }
                    }

                    // Discord notification
                    if let Some(ref dc_id) = user.discord_id {
                        if let Some(ref dc_http) = discord_http {
                            if let Ok(uid) = dc_id.parse::<u64>() {
                                if let Ok(dc_user) = UserId::new(uid).to_user(dc_http).await {
                                    let mut msg = CreateMessage::new().content(&message);
                                    if !download_link.is_empty() {
                                        let row =
                                            CreateActionRow::Buttons(vec![CreateButton::new_link(
                                                &download_link,
                                            )
                                            .label("Download")]);
                                        msg = msg.components(vec![row]);
                                    }
                                    if let Ok(sent_msg) = dc_user.dm(dc_http, msg).await {
                                        let _ = crate::db::associate_message_with_torrent(
                                            &sent_msg.id.to_string(),
                                            &torrent.hash,
                                            None,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Pause seeding policy
            let mut should_pause = false;
            let seed_policy = settings.seed_after_download.as_str();
            if seed_policy == "never" {
                should_pause = true;
            } else if seed_policy == "admin_only" {
                let has_admin = target_users.iter().any(|u| u.role == "administrator");
                if !has_admin {
                    should_pause = true;
                }
            }

            let is_uploaded_and_deleted = settings.s3.enabled && settings.s3.mode == "upload";
            if should_pause && !is_uploaded_and_deleted {
                if let Err(e) = manager.pause(&torrent.hash).await {
                    eprintln!(
                        "[Tasks] Failed to pause torrent \"{}\": {:?}",
                        torrent.name, e
                    );
                } else {
                    println!(
                        "[Tasks] Paused completed torrent \"{}\" to stop seeding per policy: {}",
                        torrent.name, seed_policy
                    );
                }
            }

            // Save to Redis and SQLite
            redis.set(&torrent.hash, "true", Some(10 * 86400)).await;
            let _ = mark_notification_sent(&torrent.hash);
        }
    }
}

/// Checks all downloading torrents and edits their notification messages
/// in-place whenever they cross a new progress milestone.
pub async fn torrent_progress_update(
    tg_bot: Option<&Bot>,
    discord_http: Option<Arc<serenity::http::Http>>,
    redis: &RedisWrapper,
    settings: &Settings,
    manager: &dyn TorrentClient,
) {
    let interval = settings
        .notifications
        .progress_report_interval
        .max(1)
        .min(100);
    let min_bytes = (settings.notifications.min_size_gb * 1_073_741_824.0) as u64;

    let downloading = match manager.get_torrents(None, Some("downloading")).await {
        Ok(t) => t,
        Err(_) => return,
    };

    for torrent in downloading {
        // Skip small files
        if torrent.size < min_bytes {
            continue;
        }

        let progress_percent = (torrent.progress * 100.0).floor() as u32;
        // Current milestone bucket (e.g. 23% with interval=10 -> milestone 20)
        let milestone = (progress_percent / interval) * interval;

        // Skip 0% or 100% (completion handled by torrent_finished)
        if milestone == 0 || progress_percent >= 100 {
            continue;
        }

        let redis_key = format!("progress_notified:{}", torrent.hash);
        let last_reported: u32 = redis
            .get(&redis_key)
            .await
            .unwrap_or_default()
            .parse()
            .unwrap_or(0);

        if milestone <= last_reported {
            continue; // Already reported this milestone
        }

        // Build the updated message text
        let progress_bar = format_progress(torrent.progress, 20);
        let size_str = convert_size(torrent.size);
        let speed_str = convert_size(torrent.dlspeed);

        let state_cap = {
            let mut chars = torrent.state.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        };

        // Telegram update
        if let Some(bot) = tg_bot {
            if let Ok(entries) = get_messages_for_torrent(&torrent.hash) {
                for (msg_id_str, chat_id_str) in &entries {
                    if let (Ok(msg_id_i), Some(chat_id_s)) =
                        (msg_id_str.parse::<i32>(), chat_id_str)
                    {
                        if let Ok(chat_id_i) = chat_id_s.parse::<i64>() {
                            let tg_text = format!(
                                "✅ *{}*\n{}{}\n*State:* `{}` \\| *Size:* `{}` \\| *Speed:* `{}/s`\n*Hash:* `{}`",
                                escape_markdown(&torrent.name),
                                escape_markdown(&progress_bar),
                                progress_percent,
                                escape_markdown(&state_cap),
                                escape_markdown(&size_str),
                                escape_markdown(&speed_str),
                                escape_markdown(&torrent.hash),
                            );
                            let _ = bot
                                .edit_message_text(
                                    teloxide::types::ChatId(chat_id_i),
                                    teloxide::types::MessageId(msg_id_i),
                                    tg_text,
                                )
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await;
                        }
                    }
                }
            }
        }

        // Discord update — post a new message to the DM channel (interaction edits expire)
        if let Some(ref dc_http) = discord_http {
            if let Ok(entries) = get_messages_for_torrent(&torrent.hash) {
                for (_, chat_id_str) in &entries {
                    if let Some(channel_id_s) = chat_id_str {
                        // channel_id_s may be a Discord channel/DM id prefixed "dc:"
                        if let Some(raw_id) = channel_id_s.strip_prefix("dc:") {
                            if let Ok(channel_id) = raw_id.parse::<u64>() {
                                let dc_text = format!(
                                    "**{}**\n{}{}\nState: `{}` | Size: `{}` | Speed: `{}/s`\nHash: `{}`",
                                    torrent.name,
                                    progress_bar,
                                    progress_percent,
                                    state_cap,
                                    size_str,
                                    speed_str,
                                    torrent.hash,
                                );
                                use serenity::model::id::ChannelId;
                                let ch = ChannelId::new(channel_id);
                                let _ = ch.say(dc_http, dc_text).await;
                            }
                        }
                    }
                }
            }
        }

        // Record the milestone in Redis (keep for 30 days)
        redis
            .set(&redis_key, &milestone.to_string(), Some(30 * 86400))
            .await;
    }
}

pub fn watch_config(settings: Arc<RwLock<Settings>>) {
    tokio::spawn(async move {
        let path = std::path::Path::new("data/config.yml");
        let mut last_modified = None;

        loop {
            if path.exists() {
                if let Ok(metadata) = std::fs::metadata(path) {
                    if let Ok(modified) = metadata.modified() {
                        if let Some(last) = last_modified {
                            if modified > last {
                                println!("Config file change detected. Reloading settings...");
                                let new_settings = Settings::load_settings();
                                {
                                    let mut s_write = settings.write().await;
                                    *s_write = new_settings;
                                }
                                println!(
                                    "Settings reloaded successfully due to config file change"
                                );
                            }
                        }
                        last_modified = Some(modified);
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

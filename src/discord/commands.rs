use serenity::builder::CreateCommand;
use serenity::builder::CreateCommandOption;
use serenity::builder::{
    CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage,
    EditInteractionResponse,
};
use serenity::model::application::CommandInteraction;
use serenity::model::application::CommandOptionType;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::collections::HashMap;

use super::state::{translate, Handler};
use super::views;
use crate::config::{Settings, UserSettings};
use crate::utils::{convert_size, extract_hash_from_magnet, read_cpu_temp};

pub async fn register_commands(ctx: &Context, ready: &Ready) {
    println!("Discord bot is online! Logged in as {}", ready.user.name);

    let guilds = &ready.guilds;

    let list_cmd = CreateCommand::new("list").description("List active torrents and manage them");
    let stats_cmd = CreateCommand::new("stats").description("Get host system statistics");
    let speedlimit_cmd =
        CreateCommand::new("speedlimit").description("Toggle alternate speed limits mode");
    let seeding_cmd = CreateCommand::new("seeding")
        .description("Set seeding policy after download completes (Admin only)")
        .add_option(
            CreateCommandOption::new(CommandOptionType::String, "policy", "Seeding policy")
                .required(false)
                .add_string_choice("Always", "always")
                .add_string_choice("Never", "never")
                .add_string_choice("Admin Only", "admin_only"),
        );
    let add_cmd = CreateCommand::new("add")
        .description("Add a magnet link or torrent file URL")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "link",
                "Magnet link or torrent file URL",
            )
            .required(true),
        );
    let info_cmd = CreateCommand::new("info")
        .description("Get details of a torrent by hash or message ID")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "identifier",
                "Torrent Hash or Message ID",
            )
            .required(true),
        );
    let link_cmd = CreateCommand::new("link")
        .description("Get S3 download link of a completed torrent by hash or message ID")
        .add_option(
            CreateCommandOption::new(
                CommandOptionType::String,
                "identifier",
                "Torrent Hash or Message ID",
            )
            .required(true),
        );
    let info_menu =
        CreateCommand::new("Torrent Info").kind(serenity::model::application::CommandType::Message);
    let link_menu = CreateCommand::new("Get Download Link")
        .kind(serenity::model::application::CommandType::Message);

    for guild in guilds {
        println!("Registering guild commands for guild ID: {}", guild.id);
        let _ = guild
            .id
            .set_commands(
                &ctx.http,
                vec![
                    list_cmd.clone(),
                    stats_cmd.clone(),
                    speedlimit_cmd.clone(),
                    seeding_cmd.clone(),
                    add_cmd.clone(),
                    info_cmd.clone(),
                    link_cmd.clone(),
                    info_menu.clone(),
                    link_menu.clone(),
                ],
            )
            .await;
    }
    println!("Discord slash commands registered successfully.");
}

pub async fn handle_command(
    handler: &Handler,
    ctx: &Context,
    cmd: &CommandInteraction,
    user: &UserSettings,
    settings: &Settings,
) {
    let name = cmd.data.name.as_str();
    let interaction = serenity::model::application::Interaction::Command(cmd.clone());
    match cmd.data.kind {
        serenity::model::application::CommandType::ChatInput => {
            match name {
                "list" => {
                    if let Err(e) =
                        views::show_torrent_list(handler, ctx, &interaction, user, None).await
                    {
                        let _ = cmd
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("❌ Error: {}", e))
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                    }
                }
                "stats" => {
                    let _ = cmd.defer(&ctx.http).await;
                    // Get CPU temperature, usage, disk, memory via sysinfo
                    let mut sys = sysinfo::System::new_all();
                    sys.refresh_all();

                    let cpu_usage = sys.global_cpu_info().cpu_usage().round() as i32;

                    // Disk size
                    let mut disk_used = 0;
                    let mut disk_total = 0;
                    let mut disk_percent = 0;
                    let disks = sysinfo::Disks::new_with_refreshed_list();
                    if let Some(disk) = disks
                        .iter()
                        .find(|d| d.mount_point().to_str() == Some("/mnt"))
                        .or_else(|| disks.iter().next())
                    {
                        disk_total = disk.total_space();
                        disk_used = disk_total - disk.available_space();
                        if disk_total > 0 {
                            disk_percent =
                                ((disk_used as f64 / disk_total as f64) * 100.0).round() as i32;
                        }
                    }

                    let total_mem = sys.total_memory();
                    let free_mem = sys.available_memory();
                    let mem_percent = if total_mem > 0 {
                        (((total_mem - free_mem) as f64 / total_mem as f64) * 100.0).round() as i32
                    } else {
                        0
                    };

                    let mut vars = HashMap::new();
                    vars.insert("cpu_usage".to_string(), cpu_usage.to_string());
                    vars.insert("cpu_temp".to_string(), read_cpu_temp());
                    vars.insert("free_memory".to_string(), convert_size(free_mem));
                    vars.insert("total_memory".to_string(), convert_size(total_mem));
                    vars.insert("memory_percent".to_string(), mem_percent.to_string());
                    vars.insert("disk_used".to_string(), convert_size(disk_used));
                    vars.insert("disk_total".to_string(), convert_size(disk_total));
                    vars.insert("disk_percent".to_string(), disk_percent.to_string());

                    let stats_text = translate(user, "**============SYSTEM============**\n**CPU Usage:** {cpu_usage}%\n**CPU Temp:** {cpu_temp}°C\n**Free Memory:** {free_memory} of {total_memory} ({memory_percent}%)\n**Disks usage:** {disk_used} of {disk_total} ({disk_percent}%)", Some(&vars));

                    let seed_policy = settings.seed_after_download.as_str();
                    let seed_policy_label = match seed_policy {
                        "always" => "🌱 Always",
                        "never" => "🚫 Never",
                        "admin_only" => "👑 Admin Only",
                        _ => seed_policy,
                    };

                    let embed = serenity::builder::CreateEmbed::new()
                        .title(translate(user, "System Statistics", None))
                        .color(0x00ae86)
                        .description(stats_text)
                        .field("🌿 Seeding Policy", seed_policy_label, true);

                    let _ = cmd
                        .edit_response(&ctx.http, EditInteractionResponse::new().embed(embed))
                        .await;
                }
                "speedlimit" => {
                    if user.role == "reader" {
                        let _ = cmd
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(translate(
                                            user,
                                            "You are not authorized to use this bot",
                                            None,
                                        ))
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }
                    let _ = cmd.defer(&ctx.http).await;
                    match handler.manager.toggle_speed_limit().await {
                        Ok(active) => {
                            let mode_str = if active { "ON" } else { "OFF" };
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content(format!(
                                        "Alternate speed limits toggled: **{}**",
                                        mode_str
                                    )),
                                )
                                .await;
                        }
                        Err(e) => {
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content(format!("Error: {}", e)),
                                )
                                .await;
                        }
                    }
                }
                "seeding" => {
                    if user.role != "administrator" {
                        let _ = cmd
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(translate(
                                            user,
                                            "You are not authorized to use this bot",
                                            None,
                                        ))
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }

                    let policy_opt = cmd
                        .data
                        .options
                        .iter()
                        .find(|o| o.name == "policy")
                        .and_then(|o| o.value.as_str());
                    match policy_opt {
                        None => {
                            let current = settings.seed_after_download.as_str();
                            let label = match current {
                                "always" => "🌱 Always (seed after every download)",
                                "never" => "🚫 Never (stop seeding immediately)",
                                "admin_only" => "👑 Admin Only (seed only when an admin downloads)",
                                _ => current,
                            };
                            let _ = cmd
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content(format!(
                                                "**Current seeding policy:** {}",
                                                label
                                            ))
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                        }
                        Some(policy) => {
                            let prev = settings.seed_after_download.as_str();
                            {
                                let mut settings_write = handler.settings.write().await;
                                settings_write.seed_after_download = policy.to_string();
                                settings_write.export_settings();
                            }
                            let label_prev = match prev {
                                "always" => "🌱 Always",
                                "never" => "🚫 Never",
                                "admin_only" => "👑 Admin Only",
                                _ => prev,
                            };
                            let label_new = match policy {
                                "always" => "🌱 Always",
                                "never" => "🚫 Never",
                                "admin_only" => "👑 Admin Only",
                                _ => policy,
                            };
                            let _ = cmd
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content(format!(
                                                "✅ Seeding policy updated:\n**{}** → **{}**",
                                                label_prev, label_new
                                            ))
                                            .ephemeral(true),
                                    ),
                                )
                                .await;
                        }
                    }
                }
                "add" => {
                    if user.role == "reader" {
                        let _ = cmd
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(translate(
                                            user,
                                            "You are not authorized to use this bot",
                                            None,
                                        ))
                                        .ephemeral(true),
                                ),
                            )
                            .await;
                        return;
                    }

                    let _ = cmd.defer(&ctx.http).await;

                    let link_opt = cmd
                        .data
                        .options
                        .iter()
                        .find(|o| o.name == "link")
                        .and_then(|o| o.value.as_str())
                        .unwrap_or("");

                    let before = handler
                        .manager
                        .get_torrents(None, None)
                        .await
                        .unwrap_or_default();

                    let result = if link_opt.to_lowercase().starts_with("magnet:?xt=urn:") {
                        match handler.manager.add_magnet(link_opt, None).await {
                            Ok(true) => {
                                let hash_opt = extract_hash_from_magnet(link_opt);
                                let mut details_msg =
                                    "✅ **Magnet link added successfully!**".to_string();
                                if let Some(ref h) = hash_opt {
                                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                    if let Ok(Some(t)) = handler.manager.get_torrent(h, None).await
                                    {
                                        let peers_line = match (t.num_seeds, t.num_peers) {
                                            (None, None) => String::new(),
                                            (Some(seeds), Some(peers)) => format!(
                                                "\n**Peers:** Seeds: `{}` | Peers: `{}`",
                                                seeds, peers
                                            ),
                                            (Some(seeds), None) => {
                                                format!("\n**Peers:** Seeds: `{}`", seeds)
                                            }
                                            (None, Some(peers)) => {
                                                format!("\n**Peers:** Peers: `{}`", peers)
                                            }
                                        };
                                        details_msg = format!("✅ **Magnet link added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`{}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_line, t.hash);
                                    }
                                }
                                Ok((details_msg, hash_opt))
                            }
                            Ok(false) => Err("❌ Failed to add magnet link.".to_string()),
                            Err(e) => {
                                if e.contains("409") {
                                    Err("⚠️ This torrent/magnet link is already in the download list.".to_string())
                                } else {
                                    Err(format!("❌ Error: {}", e))
                                }
                            }
                        }
                    } else if link_opt.to_lowercase().starts_with("http://")
                        || link_opt.to_lowercase().starts_with("https://")
                    {
                        let filename = link_opt.split('/').next_back().unwrap_or("downloaded.torrent");
                        let clean_filename = if filename.ends_with(".torrent") {
                            filename
                        } else {
                            "downloaded.torrent"
                        };

                        match reqwest::get(link_opt).await {
                            Ok(resp) => {
                                match resp.bytes().await {
                                    Ok(bytes) => {
                                        match handler
                                            .manager
                                            .add_torrent(bytes.to_vec(), clean_filename, None)
                                            .await
                                        {
                                            Ok(true) => {
                                                tokio::time::sleep(
                                                    std::time::Duration::from_millis(500),
                                                )
                                                .await;
                                                let after = handler
                                                    .manager
                                                    .get_torrents(None, None)
                                                    .await
                                                    .unwrap_or_default();
                                                let new_torrent = after.iter().find(|t_after| {
                                                    !before.iter().any(|t_before| {
                                                        t_before.hash == t_after.hash
                                                    })
                                                });
                                                let (details_msg, hash_opt) = if let Some(t) =
                                                    new_torrent
                                                {
                                                    let peers_line = match (t.num_seeds, t.num_peers) {
                                                    (None, None) => String::new(),
                                                    (Some(seeds), Some(peers)) => format!("\n**Peers:** Seeds: `{}` | Peers: `{}`", seeds, peers),
                                                    (Some(seeds), None) => format!("\n**Peers:** Seeds: `{}`", seeds),
                                                    (None, Some(peers)) => format!("\n**Peers:** Peers: `{}`", peers),
                                                };
                                                    (format!("✅ **Torrent file added successfully!**\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`{}\n**Hash:** `{}`", t.name, convert_size(t.size), t.state, peers_line, t.hash), Some(t.hash.clone()))
                                                } else {
                                                    (
                                                        "✅ Torrent file added successfully!"
                                                            .to_string(),
                                                        None,
                                                    )
                                                };
                                                Ok((details_msg, hash_opt))
                                            }
                                            Ok(false) => {
                                                Err("❌ Failed to add torrent file.".to_string())
                                            }
                                            Err(e) => {
                                                if e.contains("409") {
                                                    Err("⚠️ This torrent/magnet link is already in the download list.".to_string())
                                                } else {
                                                    Err(format!("❌ Error: {}", e))
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => Err(format!("Failed to read URL content: {}", e)),
                                }
                            }
                            Err(e) => Err(format!("Failed to download from URL: {}", e)),
                        }
                    } else {
                        Err("❌ Invalid link format. Must be a magnet link or http/https torrent URL.".to_string())
                    };

                    match result {
                        Ok((msg, hash_opt)) => {
                            if let Ok(resp_msg) = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content(&msg),
                                )
                                .await
                            {
                                if let Some(h) = hash_opt {
                                    // Store channel_id with dc: prefix for progress tracking
                                    let channel_id_str = format!("dc:{}", resp_msg.channel_id);
                                    let _ = crate::db::associate_message_with_torrent(
                                        &resp_msg.id.to_string(),
                                        &h,
                                        Some(&channel_id_str),
                                    );
                                    let manager = handler.manager.clone();
                                    let http = ctx.http.clone();
                                    let cmd_clone = cmd.clone();
                                    let msg_content = msg.clone();
                                    tokio::spawn(async move {
                                        for _ in 0..15 {
                                            tokio::time::sleep(std::time::Duration::from_millis(
                                                1500,
                                            ))
                                            .await;
                                            if let Ok(Some(t)) = manager.get_torrent(&h, None).await
                                            {
                                                if t.size > 0 && t.name != h && !t.name.is_empty() {
                                                    let peers_str = match (t.num_seeds, t.num_peers)
                                                    {
                                                        (Some(seeds), Some(peers)) => format!(
                                                            "Seeds: `{}` | Peers: `{}`",
                                                            seeds, peers
                                                        ),
                                                        (None, Some(peers)) => {
                                                            format!("Peers: `{}`", peers)
                                                        }
                                                        (Some(seeds), None) => {
                                                            format!("Seeds: `{}`", seeds)
                                                        }
                                                        (None, None) => "Unknown".to_string(),
                                                    };
                                                    let prefix = if msg_content
                                                        .contains("Magnet link")
                                                    {
                                                        "✅ **Magnet link added successfully!**"
                                                    } else {
                                                        "✅ **Torrent file added successfully!**"
                                                    };
                                                    let updated_msg = format!("{}\n\n**Name:** {}\n**Size:** {}\n**Status:** `{}`\n**Peers:** {}\n**Hash:** `{}`", prefix, t.name, convert_size(t.size), t.state, peers_str, t.hash);
                                                    let _ = cmd_clone
                                                        .edit_response(
                                                            &http,
                                                            EditInteractionResponse::new()
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
                        Err(msg) => {
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content(msg),
                                )
                                .await;
                        }
                    }
                }
                "info" => {
                    let _ = cmd.defer(&ctx.http).await;
                    let identifier = cmd
                        .data
                        .options
                        .iter()
                        .find(|o| o.name == "identifier")
                        .and_then(|o| o.value.as_str())
                        .unwrap_or("");

                    let mut hash = identifier.to_string();
                    if let Ok(Some(h)) = crate::db::get_torrent_hash_for_message(identifier) {
                        hash = h;
                    }

                    let _ = views::show_torrent_details(
                        handler,
                        ctx,
                        &interaction,
                        user,
                        &hash,
                        settings,
                    )
                    .await;
                }
                "link" => {
                    let _ = cmd.defer_ephemeral(&ctx.http).await;
                    let identifier = cmd
                        .data
                        .options
                        .iter()
                        .find(|o| o.name == "identifier")
                        .and_then(|o| o.value.as_str())
                        .unwrap_or("");

                    let mut hash = identifier.to_string();
                    if let Ok(Some(h)) = crate::db::get_torrent_hash_for_message(identifier) {
                        hash = h;
                    }

                    match handler.manager.get_torrent(&hash, None).await {
                        Ok(Some(torrent)) => {
                            if torrent.progress >= 1.0 {
                                match crate::s3::get_download_link(settings, &torrent).await {
                                    Ok(Some(url)) => {
                                        let row =
                                            CreateActionRow::Buttons(vec![CreateButton::new_link(
                                                url,
                                            )
                                            .label("Download")]);
                                        let content = if settings.local_server.enabled {
                                            format!(
                                                "🔗 Here is your download link for **{}**:",
                                                torrent.name
                                            )
                                        } else {
                                            format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", torrent.name)
                                        };
                                        let _ = cmd
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content(content)
                                                    .components(vec![row]),
                                            )
                                            .await;
                                    }
                                    Ok(None) => {
                                        let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Download links are either disabled or not configured.")).await;
                                    }
                                    Err(e) => {
                                        let _ = cmd
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content(format!("Error: {}", e)),
                                            )
                                            .await;
                                    }
                                }
                            } else {
                                let _ = cmd
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new()
                                            .content("❌ Torrent is not fully completed yet."),
                                    )
                                    .await;
                            }
                        }
                        _ => {
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content("❌ Torrent not found."),
                                )
                                .await;
                        }
                    }
                }
                _ => {}
            }
        }
        serenity::model::application::CommandType::Message => {
            let target_msg_id = cmd.data.target_id.unwrap().to_string();
            match name {
                "Torrent Info" => {
                    let _ = cmd.defer(&ctx.http).await;
                    match crate::db::get_torrent_hash_for_message(&target_msg_id) {
                        Ok(Some(hash)) => {
                            let _ = views::show_torrent_details(
                                handler,
                                ctx,
                                &interaction,
                                user,
                                &hash,
                                settings,
                            )
                            .await;
                        }
                        Ok(None) => {
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content(
                                        "❌ This message is not associated with any torrent.",
                                    ),
                                )
                                .await;
                        }
                        Err(e) => {
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content(format!("❌ Database error: {}", e)),
                                )
                                .await;
                        }
                    }
                }
                "Get Download Link" => {
                    let _ = cmd.defer_ephemeral(&ctx.http).await;
                    match crate::db::get_torrent_hash_for_message(&target_msg_id) {
                        Ok(Some(hash)) => match handler.manager.get_torrent(&hash, None).await {
                            Ok(Some(torrent)) => {
                                if torrent.progress >= 1.0 {
                                    match crate::s3::get_download_link(settings, &torrent).await {
                                        Ok(Some(url)) => {
                                            let row = CreateActionRow::Buttons(vec![
                                                CreateButton::new_link(url).label("Download"),
                                            ]);
                                            let content = if settings.local_server.enabled {
                                                format!(
                                                    "🔗 Here is your download link for **{}**:",
                                                    torrent.name
                                                )
                                            } else {
                                                format!("🔗 Here is your temporary download link for **{}**:\n*(Expires in 1 hour)*", torrent.name)
                                            };
                                            let _ = cmd
                                                .edit_response(
                                                    &ctx.http,
                                                    EditInteractionResponse::new()
                                                        .content(content)
                                                        .components(vec![row]),
                                                )
                                                .await;
                                        }
                                        Ok(None) => {
                                            let _ = cmd.edit_response(&ctx.http, EditInteractionResponse::new().content("❌ Download links are either disabled or not configured.")).await;
                                        }
                                        Err(e) => {
                                            let _ = cmd
                                                .edit_response(
                                                    &ctx.http,
                                                    EditInteractionResponse::new()
                                                        .content(format!("Error: {}", e)),
                                                )
                                                .await;
                                        }
                                    }
                                } else {
                                    let _ = cmd
                                        .edit_response(
                                            &ctx.http,
                                            EditInteractionResponse::new()
                                                .content("❌ Torrent is not fully completed yet."),
                                        )
                                        .await;
                                }
                            }
                            _ => {
                                let _ = cmd
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new()
                                            .content("❌ Torrent not found in client."),
                                    )
                                    .await;
                            }
                        },
                        Ok(None) => {
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content(
                                        "❌ This message is not associated with any torrent.",
                                    ),
                                )
                                .await;
                        }
                        Err(e) => {
                            let _ = cmd
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content(format!("❌ Database error: {}", e)),
                                )
                                .await;
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

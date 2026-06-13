use std::collections::HashMap;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use teloxide::Bot;
use teloxide::net::Download;

use crate::utils::{convert_size, convert_eta, format_progress, escape_markdown, extract_hash_from_magnet, extract_clean_magnet, read_cpu_temp};
use super::state::{BotState, get_user_and_check_auth, translate};
use super::menu::send_menu;

pub async fn handle_message(bot: Bot, msg: Message, state: BotState) -> ResponseResult<()> {
    let settings = state.settings.read().await.clone();
    let user = match get_user_and_check_auth(&bot, &settings, &msg).await {
        Some(u) => u,
        None => return Ok(()),
    };

    // 0. Check if this is a reply referencing a message
    if let Some(ref_msg) = msg.reply_to_message() {
        if let Some(text) = msg.text() {
            let cmd_text = text.trim().to_lowercase();
            if cmd_text == "info" || cmd_text == "/info" || cmd_text == "link" || cmd_text == "/link" {
                let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await;
                match crate::db::get_torrent_hash_for_message(&ref_msg.id.to_string()) {
                    Ok(Some(hash)) => {
                        if cmd_text.contains("info") {
                            match state.manager.get_torrent(&hash, None).await {
                                Ok(Some(t)) => {
                                    let progress_percent = (t.progress * 100.0).round() as i32;
                                    let peers_str = match (t.num_seeds, t.num_peers) {
                                        (Some(seeds), Some(peers)) => format!("Seeds: {} | Peers: {}", seeds, peers),
                                        (None, Some(peers)) => format!("Peers: {}", peers),
                                        (Some(seeds), None) => format!("Seeds: {}", seeds),
                                        (None, None) => "Unknown".to_string(),
                                    };
                                    let details = format!(
                                        "📌 *{}*\nProgress: {} {}%\nState: `{}`\nSize: `{}`\nSpeed: `{}/s`\nETA: `{}`\nPeers: `{}`\nHash: `{}`",
                                        escape_markdown(&t.name),
                                        escape_markdown(&format_progress(t.progress, 15)),
                                        progress_percent,
                                        escape_markdown(&t.state),
                                        escape_markdown(&convert_size(t.size)),
                                        escape_markdown(&convert_size(t.dlspeed)),
                                        escape_markdown(&convert_eta(t.eta)),
                                        escape_markdown(&peers_str),
                                        escape_markdown(&t.hash)
                                    );
                                    let _ = bot.send_message(msg.chat.id, details).reply_to_message_id(msg.id).parse_mode(teloxide::types::ParseMode::MarkdownV2).await;
                                }
                                _ => {
                                    let _ = bot.send_message(msg.chat.id, "❌ Torrent not found in client.").reply_to_message_id(msg.id).await;
                                }
                            }
                        } else {
                            match state.manager.get_torrent(&hash, None).await {
                                Ok(Some(t)) => {
                                    if t.progress >= 1.0 {
                                        match crate::s3::get_download_link(&settings, &t).await {
                                            Ok(Some(url)) => {
                                                let details = if settings.local_server.enabled {
                                                    format!("🔗 Here is your download link for *{}*:\n{}", escape_markdown(&t.name), escape_markdown(&url))
                                                } else {
                                                    format!("🔗 Here is your temporary download link for *{}*:\n{}\n*(Expires in 1 hour)*", escape_markdown(&t.name), escape_markdown(&url))
                                                };
                                                let _ = bot.send_message(msg.chat.id, details).reply_to_message_id(msg.id).parse_mode(teloxide::types::ParseMode::MarkdownV2).await;
                                            }
                                            Ok(None) => {
                                                let _ = bot.send_message(msg.chat.id, "❌ Download links are either disabled or not configured.").reply_to_message_id(msg.id).await;
                                            }
                                            Err(e) => {
                                                let _ = bot.send_message(msg.chat.id, format!("❌ Error: {}", e)).reply_to_message_id(msg.id).await;
                                            }
                                        }
                                    } else {
                                        let _ = bot.send_message(msg.chat.id, "❌ Torrent is not fully completed yet.").reply_to_message_id(msg.id).await;
                                    }
                                }
                                _ => {
                                    let _ = bot.send_message(msg.chat.id, "❌ Torrent not found in client.").reply_to_message_id(msg.id).await;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        let _ = bot.send_message(msg.chat.id, "❌ This message is not associated with any torrent.").reply_to_message_id(msg.id).await;
                    }
                    Err(e) => {
                        let _ = bot.send_message(msg.chat.id, format!("❌ Database error: {}", e)).reply_to_message_id(msg.id).await;
                    }
                }
                return Ok(());
            }
        }
    }

    let text = match msg.text() {
        Some(t) => t,
        None => {
            // Check for document torrent file additions
            if let Some(doc) = msg.document() {
                if doc.file_name.as_ref().map_or(false, |name| name.ends_with(".torrent")) {
                    if user.role == "reader" {
                        let _ = bot.send_message(msg.chat.id, translate(&user, "You are not authorized to use this bot", None)).reply_to_message_id(msg.id).await;
                        return Ok(());
                    }

                    // Determine if there is a pending category action
                    let user_id = user.user_id;
                    let action_val = state.redis.get(&format!("action:{}", user_id)).await.unwrap_or_default();
                    
                    let category = if action_val.starts_with("torrent#") {
                        let cat = action_val.trim_start_matches("torrent#");
                        if cat == "None" { None } else { Some(cat) }
                    } else {
                        None
                    };

                    let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await;

                    match bot.get_file(&doc.file.id).await {
                        Ok(file) => {
                            // Download file
                            let mut bytes = Vec::new();
                            if let Ok(_) = bot.download_file(&file.path, &mut bytes).await {
                                let filename = doc.file_name.clone().unwrap_or_else(|| "torrent.torrent".to_string());
                                let before = state.manager.get_torrents(None, None).await.unwrap_or_default();
                                match state.manager.add_torrent(bytes, &filename, category).await {
                                    Ok(true) => {
                                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                        let after = state.manager.get_torrents(None, None).await.unwrap_or_default();
                                        let new_torrent = after.iter().find(|t_after| !before.iter().any(|t_before| t_before.hash == t_after.hash));
                                        let details_msg = if let Some(t) = new_torrent {
                                            let peers_line = match (t.num_seeds, t.num_peers) {
                                                (None, None) => String::new(),
                                                (Some(seeds), Some(peers)) => format!("\n*Peers:* {}", escape_markdown(&format!("Seeds: {} | Peers: {}", seeds, peers))),
                                                (Some(seeds), None) => format!("\n*Peers:* {}", escape_markdown(&format!("Seeds: {}", seeds))),
                                                (None, Some(peers)) => format!("\n*Peers:* {}", escape_markdown(&format!("Peers: {}", peers))),
                                            };
                                            format!("✅ *Torrent file added successfully\\!*\n\n*Name:* {}\n*Size:* {}\n*Status:* `{}`{}\n*Hash:* `{}`", escape_markdown(&t.name), escape_markdown(&convert_size(t.size)), escape_markdown(&t.state), peers_line, escape_markdown(&t.hash))
                                        } else {
                                            "✅ *Torrent file added successfully\\!*".to_string()
                                        };
                                        if let Ok(sent_msg) = bot.send_message(msg.chat.id, details_msg).reply_to_message_id(msg.id).parse_mode(teloxide::types::ParseMode::MarkdownV2).await {
                                            if let Some(t) = new_torrent {
                                                let chat_id_str = msg.chat.id.0.to_string();
                                                let _ = crate::db::associate_message_with_torrent(&sent_msg.id.to_string(), &t.hash, Some(&chat_id_str));
                                            }
                                        }
                                    }
                                    Ok(false) => {
                                        let keyboard = InlineKeyboardMarkup::new(vec![vec![
                                            InlineKeyboardButton::callback("Retry", "dc_retry")
                                        ]]);
                                        let _ = bot.send_message(msg.chat.id, "❌ Failed to add torrent file.").reply_to_message_id(msg.id).reply_markup(keyboard).await;
                                    }
                                    Err(e) => {
                                        let content = if e.contains("409") {
                                            "⚠️ This torrent/magnet link is already in the download list.".to_string()
                                        } else {
                                            format!("❌ Error: {}", e)
                                        };
                                        let keyboard = InlineKeyboardMarkup::new(vec![vec![
                                            InlineKeyboardButton::callback("Retry", "dc_retry")
                                        ]]);
                                        let _ = bot.send_message(msg.chat.id, content).reply_to_message_id(msg.id).reply_markup(keyboard).await;
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            let _ = bot.send_message(msg.chat.id, "Failed to download attachment.").reply_to_message_id(msg.id).await;
                        }
                    }
                }
            }
            return Ok(());
        }
    };

    if text.starts_with('/') {
        let cmd = text.trim_start_matches('/');
        if cmd == "start" {
            let _ = send_menu(&bot, msg.chat.id, None, &user, &state.redis).await;
        } else if cmd == "stats" {
            let mut sys = sysinfo::System::new_all();
            sys.refresh_all();
            
            let cpu_usage = sys.global_cpu_info().cpu_usage().round() as i32;
            
            let mut disk_used = 0;
            let mut disk_total = 0;
            let mut disk_percent = 0;
            let disks = sysinfo::Disks::new_with_refreshed_list();
            if let Some(disk) = disks.iter().find(|d| d.mount_point().to_str() == Some("/mnt")).or_else(|| disks.iter().next()) {
                disk_total = disk.total_space();
                disk_used = disk_total - disk.available_space();
                if disk_total > 0 {
                    disk_percent = ((disk_used as f64 / disk_total as f64) * 100.0).round() as i32;
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

            let stats_text = translate(&user, "**============SYSTEM============**\n**CPU Usage:** {cpu_usage}%\n**CPU Temp:** {cpu_temp}°C\n**Free Memory:** {free_memory} of {total_memory} ({memory_percent}%)\n**Disks usage:** {disk_used} of {disk_total} ({disk_percent}%)", Some(&vars));
            let _ = bot.send_message(msg.chat.id, stats_text).await;
        }
        return Ok(());
    }

    if user.role == "reader" {
        let _ = bot.send_message(msg.chat.id, translate(&user, "You are not authorized to use this bot", None)).reply_to_message_id(msg.id).await;
        return Ok(());
    }

    let user_id = user.user_id;
    let action_val = state.redis.get(&format!("action:{}", user_id)).await.unwrap_or_default();

    if action_val == "category_name" {
        if user.role != "administrator" { return Ok(()); }
        state.redis.set(&format!("action:{}", user_id), &format!("category_dir_add#{}", text), None).await;
        let mut vars = HashMap::new(); vars.insert("category_name".to_string(), text.to_string());
        let text_path = translate(&user, "Please, send the path for the category {category_name}", Some(&vars));
        let _ = bot.send_message(msg.chat.id, text_path).reply_to_message_id(msg.id).await;
    } else if action_val.starts_with("category_dir_add#") {
        if user.role != "administrator" { return Ok(()); }
        let cat_name = action_val.trim_start_matches("category_dir_add#");
        let _ = state.manager.create_category(cat_name, text).await;
        let _ = bot.send_message(msg.chat.id, "Category created successfully!").await;
        state.redis.set(&format!("action:{}", user_id), "", None).await;
    } else if action_val.starts_with("category_dir_modify#") {
        if user.role != "administrator" { return Ok(()); }
        let cat_name = action_val.trim_start_matches("category_dir_modify#");
        let _ = state.manager.edit_category(cat_name, text).await;
        let _ = bot.send_message(msg.chat.id, "Category path modified successfully!").await;
        state.redis.set(&format!("action:{}", user_id), "", None).await;
    } else {
        // Parse magnet links
        if let Some(magnet_link) = extract_clean_magnet(text) {

            let category = if action_val.starts_with("magnet#") {
                let cat = action_val.trim_start_matches("magnet#");
                if cat == "None" { None } else { Some(cat) }
            } else {
                None
            };

            let _ = bot.send_chat_action(msg.chat.id, teloxide::types::ChatAction::Typing).await;

            match state.manager.add_magnet(&magnet_link, category).await {
                Ok(true) => {
                    let hash_opt = extract_hash_from_magnet(&magnet_link);
                    let mut details_msg = "✅ *Magnet link added successfully!*".to_string();
                    if let Some(ref h) = hash_opt {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        if let Ok(Some(t)) = state.manager.get_torrent(h, None).await {
                            let peers_line = match (t.num_seeds, t.num_peers) {
                                (None, None) => String::new(),
                                (Some(seeds), Some(peers)) => format!("\n*Peers:* {}", escape_markdown(&format!("Seeds: {} | Peers: {}", seeds, peers))),
                                (Some(seeds), None) => format!("\n*Peers:* {}", escape_markdown(&format!("Seeds: {}", seeds))),
                                (None, Some(peers)) => format!("\n*Peers:* {}", escape_markdown(&format!("Peers: {}", peers))),
                            };
                            details_msg = format!("✅ *Magnet link added successfully\\!*\n\n*Name:* {}\n*Size:* {}\n*Status:* `{}`{}\n*Hash:* `{}`", escape_markdown(&t.name), escape_markdown(&convert_size(t.size)), escape_markdown(&t.state), peers_line, escape_markdown(&t.hash));
                        }
                    }
                    if let Ok(sent_msg) = bot.send_message(msg.chat.id, details_msg).reply_to_message_id(msg.id).parse_mode(teloxide::types::ParseMode::MarkdownV2).await {
                        if let Some(h) = hash_opt {
                            let chat_id_str = msg.chat.id.0.to_string();
                            let _ = crate::db::associate_message_with_torrent(&sent_msg.id.to_string(), &h, Some(&chat_id_str));
                            let manager = state.manager.clone();
                            let bot_clone = bot.clone();
                            let chat_id = msg.chat.id;
                            let msg_id = sent_msg.id;
                            tokio::spawn(async move {
                                for _ in 0..15 {
                                     tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                     if let Ok(Some(t)) = manager.get_torrent(&h, None).await {
                                         if t.size > 0 && t.name != h && !t.name.is_empty() {
                                             let peers_line = match (t.num_seeds, t.num_peers) {
                                                 (None, None) => String::new(),
                                                 (Some(seeds), Some(peers)) => format!("\n*Peers:* {}", escape_markdown(&format!("Seeds: {} | Peers: {}", seeds, peers))),
                                                 (Some(seeds), None) => format!("\n*Peers:* {}", escape_markdown(&format!("Seeds: {}", seeds))),
                                                 (None, Some(peers)) => format!("\n*Peers:* {}", escape_markdown(&format!("Peers: {}", peers))),
                                             };
                                             let updated_msg = format!("✅ *Magnet link added successfully\\!*\n\n*Name:* {}\n*Size:* {}\n*Status:* `{}`{}\n*Hash:* `{}`", escape_markdown(&t.name), escape_markdown(&convert_size(t.size)), escape_markdown(&t.state), peers_line, escape_markdown(&t.hash));
                                             let _ = bot_clone.edit_message_text(chat_id, msg_id, updated_msg).parse_mode(teloxide::types::ParseMode::MarkdownV2).await;
                                             break;
                                         }
                                     }
                                }
                            });
                        }
                    }
                }
                Ok(false) => {
                    let keyboard = InlineKeyboardMarkup::new(vec![vec![
                        InlineKeyboardButton::callback("Retry", "dc_retry")
                    ]]);
                    let _ = bot.send_message(msg.chat.id, "❌ Failed to add magnet link.").reply_to_message_id(msg.id).reply_markup(keyboard).await;
                }
                Err(e) => {
                    let content = if e.contains("409") {
                        "⚠️ This torrent/magnet link is already in the download list.".to_string()
                    } else {
                        format!("❌ Error: {}", e)
                    };
                    let keyboard = InlineKeyboardMarkup::new(vec![vec![
                        InlineKeyboardButton::callback("Retry", "dc_retry")
                    ]]);
                    let _ = bot.send_message(msg.chat.id, content).reply_to_message_id(msg.id).reply_markup(keyboard).await;
                }
            }
        }
    }

    Ok(())
}

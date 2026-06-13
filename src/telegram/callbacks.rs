use std::collections::HashMap;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, InputFile};
use teloxide::Bot;

use crate::config::Settings;
use super::state::{BotState, get_resolved_user, translate};
use super::menu::{send_menu, list_active_torrents, list_categories, render_client_settings, render_torrent_details};

pub async fn handle_callback_query(bot: Bot, q: CallbackQuery, state: BotState) -> ResponseResult<()> {
    let user_id = q.from.id.0 as i64;
    let settings = state.settings.read().await.clone();
    let user = get_resolved_user(&settings, user_id);

    let message = match q.message {
        Some(m) => m,
        None => return Ok(()),
    };
    let chat_id = message.chat.id;
    let message_id = message.id;

    let data = match q.data {
        Some(d) => d,
        None => return Ok(()),
    };

    let parts: Vec<&str> = data.split(':').collect();
    let prefix = parts[0];
    let arg1 = parts.get(1).cloned().unwrap_or("");
    let arg2 = parts.get(2).cloned().unwrap_or("");

    let role = &user.role;
    let is_admin = role == "administrator";
    let is_manager = role == "manager" || is_admin;

    match prefix {
        "menu" => {
            let _ = send_menu(&bot, chat_id, Some(message_id), &user, &state.redis).await;
        }
        "list" => {
            let _ = list_active_torrents(&bot, chat_id, message_id, &user, &*state.manager, None, None).await;
        }
        "by_status_list" => {
            let status = if arg1.is_empty() { None } else { Some(arg1) };
            let _ = list_active_torrents(&bot, chat_id, message_id, &user, &*state.manager, None, status).await;
        }
        "category" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let action = arg1;
            let categories = state.manager.get_categories().await.unwrap_or(None);
            
            let cb_prefix = if action == "add_magnet" { "add_magnet" } else { "add_torrent" };
            let mut keyboard_rows = Vec::new();
            if let Some(cats) = categories {
                for cat in cats {
                    keyboard_rows.push(vec![InlineKeyboardButton::callback(&cat, format!("{}:{}", cb_prefix, cat))]);
                }
            }
            keyboard_rows.push(vec![InlineKeyboardButton::callback("None", format!("{}:None", cb_prefix))]);
            keyboard_rows.push(vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")]);

            let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
            let _ = bot.edit_message_text(chat_id, message_id, translate(&user, "Choose a category:", None)).reply_markup(keyboard).await;
        }
        "add_magnet" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            state.redis.set(&format!("action:{}", user_id), &format!("magnet#{}", arg1), None).await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Send a magnet link", None)).await;

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")]]);
            let _ = bot.edit_message_text(chat_id, message_id, translate(&user, "Send a magnet link", None)).reply_markup(keyboard).await;
        }
        "add_torrent" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            state.redis.set(&format!("action:{}", user_id), &format!("torrent#{}", arg1), None).await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Send a torrent file", None)).await;

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")]]);
            let _ = bot.edit_message_text(chat_id, message_id, translate(&user, "Send a torrent file", None)).reply_markup(keyboard).await;
        }
        "menu_pause_resume" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![
                    InlineKeyboardButton::callback(translate(&user, "⏸ Pause", None), "pause:"),
                    InlineKeyboardButton::callback(translate(&user, "▶️ Resume", None), "resume:")
                ],
                vec![
                    InlineKeyboardButton::callback(translate(&user, "⏸ Pause All", None), "pause_all:"),
                    InlineKeyboardButton::callback(translate(&user, "▶️ Resume All", None), "resume_all:")
                ],
                vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")]
            ]);
            let _ = bot.edit_message_text(chat_id, message_id, translate(&user, "Pause/Resume a torrent", None)).reply_markup(keyboard).await;
        }
        "pause_all" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.pause_all().await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Paused all torrents", None)).await;
        }
        "resume_all" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.resume_all().await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Resumed all torrents", None)).await;
        }
        "pause" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            if arg1.is_empty() {
                let _ = list_active_torrents(&bot, chat_id, message_id, &user, &*state.manager, Some("pause"), None).await;
            } else {
                let _ = state.manager.pause(arg1).await;
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "Torrent Paused", None)).await;
                if let Ok(Some(t)) = state.manager.get_torrent(arg1, None).await {
                    let _ = render_torrent_details(&bot, chat_id, message_id, &user, &t).await;
                }
            }
        }
        "resume" => {
            if !is_manager {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            if arg1.is_empty() {
                let _ = list_active_torrents(&bot, chat_id, message_id, &user, &*state.manager, Some("resume"), None).await;
            } else {
                let _ = state.manager.resume(arg1).await;
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "Torrent Resumed", None)).await;
                if let Ok(Some(t)) = state.manager.get_torrent(arg1, None).await {
                    let _ = render_torrent_details(&bot, chat_id, message_id, &user, &t).await;
                }
            }
        }
        "menu_delete" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(translate(&user, "🗑 Delete", None), "delete_one:")],
                vec![InlineKeyboardButton::callback(translate(&user, "🗑 Delete All", None), "delete_all:")],
                vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")],
            ]);
            let _ = bot.edit_message_text(chat_id, message_id, "Delete a torrent").reply_markup(keyboard).await;
        }
        "delete_one" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            if arg1.is_empty() {
                let _ = list_active_torrents(&bot, chat_id, message_id, &user, &*state.manager, Some("delete_one"), None).await;
            } else {
                let keyboard = InlineKeyboardMarkup::new(vec![
                    vec![InlineKeyboardButton::callback(translate(&user, "🗑 Delete torrent", None), format!("delete_one_no_data:{}", arg1))],
                    vec![InlineKeyboardButton::callback(translate(&user, "🗑 Delete torrent and data", None), format!("delete_one_data:{}", arg1))],
                    vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")],
                ]);
                let _ = bot.edit_message_reply_markup(chat_id, message_id).reply_markup(keyboard).await;
            }
        }
        "delete_one_no_data" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.delete_one_no_data(arg1).await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Torrent deleted", None)).await;
            let _ = send_menu(&bot, chat_id, Some(message_id), &user, &state.redis).await;
        }
        "delete_one_data" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.delete_one_data(arg1).await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Torrent and data deleted", None)).await;
            let _ = send_menu(&bot, chat_id, Some(message_id), &user, &state.redis).await;
        }
        "delete_all" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(translate(&user, "🗑 Delete all torrents", None), "delete_all_no_data:")],
                vec![InlineKeyboardButton::callback(translate(&user, "🗑 Delete all torrents and data", None), "delete_all_data:")],
                vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")],
            ]);
            let _ = bot.edit_message_reply_markup(chat_id, message_id).reply_markup(keyboard).await;
        }
        "delete_all_no_data" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.delete_all_no_data().await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Deleted all torrents", None)).await;
            let _ = send_menu(&bot, chat_id, Some(message_id), &user, &state.redis).await;
        }
        "delete_all_data" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.delete_all_data().await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Deleted all torrents and data", None)).await;
            let _ = send_menu(&bot, chat_id, Some(message_id), &user, &state.redis).await;
        }
        "menu_categories" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(translate(&user, "➕ Add Category", None), "add_category:")],
                vec![InlineKeyboardButton::callback(translate(&user, "🗑 Remove Category", None), "select_category:remove_category")],
                vec![InlineKeyboardButton::callback(translate(&user, "📝 Modify Category", None), "select_category:modify_category")],
                vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")],
            ]);
            let _ = bot.edit_message_text(chat_id, message_id, translate(&user, "Pause/Resume a download", None)).reply_markup(keyboard).await;
        }
        "add_category" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            state.redis.set(&format!("action:{}", user_id), "category_name", None).await;
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "Send the category name", None)).await;

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")]]);
            let _ = bot.edit_message_text(chat_id, message_id, translate(&user, "Send the category name", None)).reply_markup(keyboard).await;
        }
        "select_category" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = list_categories(&bot, chat_id, message_id, &user, &*state.manager, arg1).await;
        }
        "remove_category" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.remove_category(arg1).await;
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")]]);
            let mut vars = HashMap::new(); vars.insert("category_name".to_string(), arg1.to_string());
            let text = translate(&user, "The category {category_name} has been removed", Some(&vars));
            let _ = bot.edit_message_text(chat_id, message_id, text).reply_markup(keyboard).await;
        }
        "modify_category" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            state.redis.set(&format!("action:{}", user_id), &format!("category_dir_modify#{}", arg1), None).await;
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")]]);
            let mut vars = HashMap::new(); vars.insert("category_name".to_string(), arg1.to_string());
            let text = translate(&user, "Send new path for category {category_name}", Some(&vars));
            let _ = bot.edit_message_text(chat_id, message_id, text).reply_markup(keyboard).await;
        }
        "settings" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(translate(&user, "📥 Client Settings", None), "edit_client:")],
                vec![InlineKeyboardButton::callback(translate(&user, "🔄 Reload Settings", None), "reload_settings:")],
                vec![InlineKeyboardButton::callback(translate(&user, "🔙 Menu", None), "menu:")],
            ]);
            let _ = bot.edit_message_text(chat_id, message_id, translate(&user, "QBittorrentBot Settings", None)).reply_markup(keyboard).await;
        }
        "edit_client" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = render_client_settings(&bot, chat_id, message_id, &user, &*state.manager, &settings).await;
        }
        "toggle_speed_limit" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let _ = state.manager.toggle_speed_limit().await;
            let _ = render_client_settings(&bot, chat_id, message_id, &user, &*state.manager, &settings).await;
        }
        "check_connection" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            match state.manager.check_connection().await {
                Ok(version) => {
                    let mut vars = HashMap::new(); vars.insert("version".to_string(), version);
                    let text = translate(&user, "✅ The connection works. QBittorrent version: {version}", Some(&vars));
                    let _ = bot.answer_callback_query(q.id).text(text).show_alert(true).await;
                }
                Err(_) => {
                    let _ = bot.answer_callback_query(q.id).text(translate(&user, "❌ Unable to establish connection with QBittorrent", None)).show_alert(true).await;
                }
            }
        }
        "reload_settings" => {
            if !is_admin {
                let _ = bot.answer_callback_query(q.id).text(translate(&user, "You are not authorized to use this bot", None)).show_alert(true).await;
                return Ok(());
            }
            let reloaded = Settings::load_settings();
            {
                let mut settings_write = state.settings.write().await;
                *settings_write = reloaded;
            }
            let _ = bot.answer_callback_query(q.id).text(translate(&user, "✅ Settings Reloaded", None)).show_alert(true).await;
        }
        "torrentInfo" => {
            if let Ok(Some(t)) = state.manager.get_torrent(arg1, None).await {
                let _ = render_torrent_details(&bot, chat_id, message_id, &user, &t).await;
            } else {
                let _ = bot.answer_callback_query(q.id).text("Torrent not found").show_alert(true).await;
            }
        }
        "export" => {
            match state.manager.export_torrent(arg1).await {
                Ok((bytes, filename)) => {
                    let _ = bot.send_document(chat_id, InputFile::memory(bytes).file_name(filename)).await;
                    let _ = bot.answer_callback_query(q.id).await;
                }
                Err(_) => {
                    let _ = bot.answer_callback_query(q.id).text("Failed to export torrent").show_alert(true).await;
                }
            }
        }
        "edit_torrent_cat" => {
            let _ = list_categories(&bot, chat_id, message_id, &user, &*state.manager, &format!("torrent_cat:{}", arg1)).await;
        }
        "torrent_cat" => {
            let _ = state.manager.set_torrents_category(arg2, arg1).await;
            let mut vars = HashMap::new(); vars.insert("category".to_string(), arg2.to_string());
            let text = translate(&user, "Torrent category changed to {category}", Some(&vars));
            let _ = bot.answer_callback_query(q.id).text(text).show_alert(true).await;
            let _ = send_menu(&bot, chat_id, Some(message_id), &user, &state.redis).await;
        }
        _ => {
            let _ = bot.answer_callback_query(q.id).await;
        }
    }

    Ok(())
}

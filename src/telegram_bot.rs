use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId};
use teloxide::Bot;
use teloxide::net::Download;

use crate::config::{Settings, UserSettings};
use crate::torrent_client::{Torrent, TorrentClient};
use crate::utils::{convert_size, convert_eta, format_progress, escape_markdown, extract_hash_from_magnet, extract_clean_magnet};
use crate::i18n::t;
use crate::redis_client::RedisWrapper;

#[derive(Clone)]
struct BotState {
    settings: Arc<RwLock<Settings>>,
    manager: Arc<dyn TorrentClient>,
    redis: RedisWrapper,
}

fn get_resolved_user(settings: &Settings, telegram_id: i64) -> UserSettings {
    settings.users.iter()
        .find(|u| u.user_id == telegram_id)
        .cloned()
        .or_else(|| {
            settings.users.iter()
                .find(|u| u.user_id == 0)
                .cloned()
        })
        .unwrap_or_else(|| {
            UserSettings {
                user_id: telegram_id,
                discord_id: None,
                role: "reader".to_string(),
                locale: Some("en".to_string()),
                notify: true,
                notification_filter: vec![],
            }
        })
}

fn translate(user: &UserSettings, key: &str, vars: Option<&HashMap<String, String>>) -> String {
    let locale = user.locale.as_deref().unwrap_or("en");
    t(key, locale, vars)
}

async fn get_user_and_check_auth(bot: &Bot, settings: &Settings, msg: &Message) -> Option<UserSettings> {
    let telegram_id = msg.from()?.id.0 as i64;
    let user = get_resolved_user(settings, telegram_id);

    // If user is not authorized (not in the list and no default/wildcard user exists)
    let is_authorized = settings.users.iter().any(|u| u.user_id == telegram_id || u.user_id == 0);
    if !is_authorized {
        let keyboard = InlineKeyboardMarkup::new(vec![vec![
            InlineKeyboardButton::url("Github", "https://github.com/ch3p4ll3/QBittorrentBot/".parse().unwrap())
        ]]);
        let txt = t("You are not authorized to use this bot", user.locale.as_deref().unwrap_or("en"), None);
        let _ = bot.send_message(msg.chat.id, txt).reply_markup(keyboard).await;
        return None;
    }

    Some(user)
}

async fn send_menu(bot: &Bot, chat_id: ChatId, message_id: Option<MessageId>, user: &UserSettings, redis: &RedisWrapper) -> Result<(), String> {
    let role = &user.role;
    let mut keyboard_rows = Vec::new();
    
    keyboard_rows.push(vec![
        InlineKeyboardButton::callback(translate(user, "📝 List", None), "list:")
    ]);

    if role == "manager" || role == "administrator" {
        keyboard_rows.push(vec![
            InlineKeyboardButton::callback(translate(user, "➕ Add Magnet", None), "category:add_magnet"),
            InlineKeyboardButton::callback(translate(user, "➕ Add Torrent", None), "category:add_torrent"),
        ]);
        keyboard_rows.push(vec![
            InlineKeyboardButton::callback(translate(user, "⏯ Pause/Resume", None), "menu_pause_resume:")
        ]);
    }

    if role == "administrator" {
        keyboard_rows.push(vec![
            InlineKeyboardButton::callback(translate(user, "🗑 Delete", None), "menu_delete:")
        ]);
        keyboard_rows.push(vec![
            InlineKeyboardButton::callback(translate(user, "📂 Categories", None), "menu_categories:")
        ]);
        keyboard_rows.push(vec![
            InlineKeyboardButton::callback(translate(user, "⚙️ Settings", None), "settings:")
        ]);
    }

    // Reset current action
    redis.set(&format!("action:{}", user.user_id), "", None).await;

    let text = translate(user, "Welcome to QBittorrent Bot", None);
    let keyboard = InlineKeyboardMarkup::new(keyboard_rows);

    if let Some(mid) = message_id {
        let _ = bot.edit_message_text(chat_id, mid, &text).reply_markup(keyboard).await;
    } else {
        let _ = bot.send_message(chat_id, &text).reply_markup(keyboard).await;
    }

    Ok(())
}

async fn list_active_torrents(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    user: &UserSettings,
    manager: &dyn TorrentClient,
    callback_prefix: Option<&str>,
    status_filter: Option<&str>,
) -> Result<(), String> {
    let clean_filter = match status_filter {
        Some("all") | None => None,
        Some(s) => Some(s),
    };

    let torrents = manager.get_torrents(None, clean_filter).await.unwrap_or_default();

    let mut keyboard_rows = Vec::new();

    let dl_mark = if clean_filter == Some("downloading") { "*" } else { "" };
    let comp_mark = if clean_filter == Some("completed") { "*" } else { "" };
    let pause_mark = if clean_filter == Some("paused") { "*" } else { "" };

    let mut vars_dl = HashMap::new(); vars_dl.insert("active".to_string(), dl_mark.to_string());
    let mut vars_comp = HashMap::new(); vars_comp.insert("active".to_string(), comp_mark.to_string());
    let mut vars_pause = HashMap::new(); vars_pause.insert("active".to_string(), pause_mark.to_string());

    keyboard_rows.push(vec![
        InlineKeyboardButton::callback(translate(user, "⏳ {active} Downloading", Some(&vars_dl)), "by_status_list:downloading"),
        InlineKeyboardButton::callback(translate(user, "✔️ {active} Completed", Some(&vars_comp)), "by_status_list:completed"),
        InlineKeyboardButton::callback(translate(user, "⏸️ {active} Paused", Some(&vars_pause)), "by_status_list:paused"),
    ]);

    let text_no_torrents = translate(user, "There are no torrents", None);
    let text_back_menu = translate(user, "🔙 Menu", None);

    if torrents.is_empty() {
        keyboard_rows.push(vec![InlineKeyboardButton::callback(text_back_menu, "menu:")]);
        let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
        let _ = bot.edit_message_text(chat_id, message_id, text_no_torrents).reply_markup(keyboard).await;
        return Ok(());
    }

    for t in torrents {
        let btn_text = if t.name.len() > 40 { format!("{}...", &t.name[..37]) } else { t.name.clone() };
        let cb_data = match callback_prefix {
            Some(prefix) => format!("{}:{}", prefix, t.hash),
            None => format!("torrentInfo:{}", t.hash),
        };
        keyboard_rows.push(vec![InlineKeyboardButton::callback(btn_text, cb_data)]);
    }

    keyboard_rows.push(vec![InlineKeyboardButton::callback(text_back_menu, "menu:")]);
    let keyboard = InlineKeyboardMarkup::new(keyboard_rows);

    let _ = bot.edit_message_reply_markup(chat_id, message_id).reply_markup(keyboard).await;
    Ok(())
}

async fn list_categories(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    user: &UserSettings,
    manager: &dyn TorrentClient,
    callback_prefix: &str,
) -> Result<(), String> {
    let categories = manager.get_categories().await.unwrap_or(None);
    let mut keyboard_rows = Vec::new();

    let text_back_menu = translate(user, "🔙 Menu", None);

    if categories.is_none() || categories.as_ref().unwrap().is_empty() {
        keyboard_rows.push(vec![InlineKeyboardButton::callback(text_back_menu, "menu:")]);
        let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
        let text_no_categories = translate(user, "There are no categories", None);
        let _ = bot.edit_message_text(chat_id, message_id, text_no_categories).reply_markup(keyboard).await;
        return Ok(());
    }

    for cat in categories.unwrap() {
        keyboard_rows.push(vec![InlineKeyboardButton::callback(&cat, format!("{}:{}", callback_prefix, cat))]);
    }

    keyboard_rows.push(vec![InlineKeyboardButton::callback(text_back_menu, "menu:")]);
    let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
    let text_choose = translate(user, "Choose a category:", None);
    let _ = bot.edit_message_text(chat_id, message_id, text_choose).reply_markup(keyboard).await;

    Ok(())
}

async fn render_client_settings(bot: &Bot, chat_id: ChatId, message_id: MessageId, user: &UserSettings, manager: &dyn TorrentClient, settings: &Settings) -> Result<(), String> {
    let speed_limit = manager.get_speed_limit_mode().await.unwrap_or(false);
    let speed_limit_status = if speed_limit { translate(user, "✅ Enabled", None) } else { translate(user, "❌ Disabled", None) };

    let mut vars = HashMap::new();
    vars.insert("speed_limit_status".to_string(), speed_limit_status);
    let confs = translate(user, "**Speed Limit**: {speed_limit_status}", Some(&vars));

    let client_type = {
        let mut chars = settings.client.r#type.chars();
        match chars.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str()
        }
    };

    let mut vars_text = HashMap::new();
    vars_text.insert("client_type".to_string(), client_type);
    vars_text.insert("configs".to_string(), confs);
    let text = translate(user, "Edit {client_type} client settings \n\n{configs}", Some(&vars_text));

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(translate(user, "🐢 Toggle Speed Limit", None), "toggle_speed_limit:")],
        vec![InlineKeyboardButton::callback(translate(user, "✅ Check Client connection", None), "check_connection:")],
        vec![InlineKeyboardButton::callback(translate(user, "🔙 Settings", None), "settings:")],
    ]);

    let _ = bot.edit_message_text(chat_id, message_id, text).reply_markup(keyboard).parse_mode(teloxide::types::ParseMode::MarkdownV2).await;
    Ok(())
}

async fn render_torrent_details(bot: &Bot, chat_id: ChatId, message_id: MessageId, user: &UserSettings, torrent: &Torrent) -> Result<(), String> {
    let mut text = format!("{}\n", escape_markdown(&torrent.name));

    if torrent.progress >= 1.0 {
        text.push_str(&t("**COMPLETED**\n", user.locale.as_deref().unwrap_or("en"), None));
    } else {
        text.push_str(&format_progress(torrent.progress, 20));
    }

    if !torrent.state.contains("stalled") {
        let current_state = {
            let mut chars = torrent.state.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str()
            }
        };
        let speed = convert_size(torrent.dlspeed);
        let mut vars_state = HashMap::new();
        vars_state.insert("current_state".to_string(), current_state);
        vars_state.insert("download_speed".to_string(), speed);
        text.push_str(&translate(user, "**State:** {current_state} \n**Download Speed:** {download_speed}/s\n", Some(&vars_state)));
    }

    let mut vars_size = HashMap::new();
    vars_size.insert("torrent_size".to_string(), convert_size(torrent.size));
    text.push_str(&translate(user, "**Size:** {torrent_size}\n", Some(&vars_size)));

    if !torrent.state.contains("stalled") {
        let mut vars_eta = HashMap::new();
        vars_eta.insert("torrent_eta".to_string(), convert_eta(torrent.eta));
        text.push_str(&translate(user, "**ETA:** {torrent_eta}\n", Some(&vars_eta)));
    }

    if let Some(ref cat) = torrent.category {
        let mut vars_cat = HashMap::new();
        vars_cat.insert("torrent_category".to_string(), cat.clone());
        text.push_str(&translate(user, "**Category:** {torrent_category}\n", Some(&vars_cat)));
    }

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(translate(user, "💾 Export torrent", None), format!("export:{}", torrent.hash))],
        vec![InlineKeyboardButton::callback(translate(user, "📝 Edit Category", None), format!("edit_torrent_cat:{}", torrent.hash))],
        vec![InlineKeyboardButton::callback(translate(user, "⏸ Pause", None), format!("pause:{}", torrent.hash))],
        vec![InlineKeyboardButton::callback(translate(user, "▶️ Resume", None), format!("resume:{}", torrent.hash))],
        vec![InlineKeyboardButton::callback(translate(user, "🗑 Delete", None), format!("delete_one:{}", torrent.hash))],
        vec![InlineKeyboardButton::callback(translate(user, "🔙 Menu", None), "menu:")],
    ]);

    let _ = bot.edit_message_text(chat_id, message_id, text).reply_markup(keyboard).parse_mode(teloxide::types::ParseMode::MarkdownV2).await;
    Ok(())
}

async fn handle_callback_query(bot: Bot, q: CallbackQuery, state: BotState) -> ResponseResult<()> {
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

async fn handle_message(bot: Bot, msg: Message, state: BotState) -> ResponseResult<()> {
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
                                            let peers_str = match (t.num_seeds, t.num_peers) {
                                                (Some(seeds), Some(peers)) => format!("Seeds: {} | Peers: {}", seeds, peers),
                                                (None, Some(peers)) => format!("Peers: {}", peers),
                                                (Some(seeds), None) => format!("Seeds: {}", seeds),
                                                (None, None) => "Unknown".to_string(),
                                            };
                                            format!("✅ *Torrent file added successfully!*\n\n*Name:* {}\n*Size:* {}\n*Status:* `{}`\n*Peers:* {}\n*Hash:* `{}`", escape_markdown(&t.name), escape_markdown(&convert_size(t.size)), escape_markdown(&t.state), escape_markdown(&peers_str), escape_markdown(&t.hash))
                                        } else {
                                            "✅ *Torrent file added successfully!*".to_string()
                                        };
                                        if let Ok(sent_msg) = bot.send_message(msg.chat.id, details_msg).reply_to_message_id(msg.id).parse_mode(teloxide::types::ParseMode::MarkdownV2).await {
                                            if let Some(t) = new_torrent {
                                                let _ = crate::db::associate_message_with_torrent(&sent_msg.id.to_string(), &t.hash);
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
            vars.insert("cpu_temp".to_string(), "0".to_string());
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
                            let peers_str = match (t.num_seeds, t.num_peers) {
                                (Some(seeds), Some(peers)) => format!("Seeds: {} | Peers: {}", seeds, peers),
                                (None, Some(peers)) => format!("Peers: {}", peers),
                                (Some(seeds), None) => format!("Seeds: {}", seeds),
                                (None, None) => "Unknown".to_string(),
                            };
                            details_msg = format!("✅ *Magnet link added successfully!*\n\n*Name:* {}\n*Size:* {}\n*Status:* `{}`\n*Peers:* {}\n*Hash:* `{}`", escape_markdown(&t.name), escape_markdown(&convert_size(t.size)), escape_markdown(&t.state), escape_markdown(&peers_str), escape_markdown(&t.hash));
                        }
                    }
                    if let Ok(sent_msg) = bot.send_message(msg.chat.id, details_msg).reply_to_message_id(msg.id).parse_mode(teloxide::types::ParseMode::MarkdownV2).await {
                        if let Some(h) = hash_opt {
                            let _ = crate::db::associate_message_with_torrent(&sent_msg.id.to_string(), &h);
                            let manager = state.manager.clone();
                            let bot_clone = bot.clone();
                            let chat_id = msg.chat.id;
                            let msg_id = sent_msg.id;
                            tokio::spawn(async move {
                                for _ in 0..15 {
                                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                                    if let Ok(Some(t)) = manager.get_torrent(&h, None).await {
                                        if t.size > 0 && t.name != h && !t.name.is_empty() {
                                            let peers_str = match (t.num_seeds, t.num_peers) {
                                                (Some(seeds), Some(peers)) => format!("Seeds: {} | Peers: {}", seeds, peers),
                                                (None, Some(peers)) => format!("Peers: {}", peers),
                                                (Some(seeds), None) => format!("Seeds: {}", seeds),
                                                (None, None) => "Unknown".to_string(),
                                            };
                                            let updated_msg = format!("✅ *Magnet link added successfully!*\n\n*Name:* {}\n*Size:* {}\n*Status:* `{}`\n*Peers:* {}\n*Hash:* `{}`", escape_markdown(&t.name), escape_markdown(&convert_size(t.size)), escape_markdown(&t.state), escape_markdown(&peers_str), escape_markdown(&t.hash));
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

pub async fn start_telegram_bot(settings: Arc<RwLock<Settings>>, redis: RedisWrapper, manager: Arc<dyn TorrentClient>) -> teloxide::Bot {
    let token = {
        let s = settings.read().await;
        s.telegram.bot_token.clone()
    };

    // Configure proxy if present
    {
        let s = settings.read().await;
        if let Some(ref proxy) = s.telegram.proxy {
            let proxy_str = Settings::get_proxy_connection_string(proxy);
            std::env::set_var("HTTPS_PROXY", &proxy_str);
            std::env::set_var("HTTP_PROXY", &proxy_str);
            println!("Configuring bot traffic proxy: {}", proxy_str);
        }
    }

    let bot = Bot::new(token);
    let state = BotState { settings, manager, redis };

    let handler = dptree::entry()
        .branch(Update::filter_callback_query().endpoint(handle_callback_query))
        .branch(Update::filter_message().endpoint(handle_message));

    let bot_clone = bot.clone();
    tokio::spawn(async move {
        Dispatcher::builder(bot_clone, handler)
            .dependencies(dptree::deps![state])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;
    });

    bot
}

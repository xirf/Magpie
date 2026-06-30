use std::collections::HashMap;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId};
use teloxide::Bot;

use super::state::translate;
use crate::config::{Settings, UserSettings};
use crate::i18n::t;
use crate::redis_client::RedisWrapper;
use crate::torrent_client::{Torrent, TorrentClient};
use crate::utils::{convert_eta, convert_size, escape_markdown, format_progress};

pub async fn send_menu(
    bot: &Bot,
    chat_id: ChatId,
    message_id: Option<MessageId>,
    user: &UserSettings,
    redis: &RedisWrapper,
) -> Result<(), String> {
    let role = &user.role;
    let mut keyboard_rows = Vec::new();

    keyboard_rows.push(vec![InlineKeyboardButton::callback(
        translate(user, "📝 List", None),
        "list:",
    )]);

    if role == "manager" || role == "administrator" {
        keyboard_rows.push(vec![
            InlineKeyboardButton::callback(
                translate(user, "➕ Add Magnet", None),
                "category:add_magnet",
            ),
            InlineKeyboardButton::callback(
                translate(user, "➕ Add Torrent", None),
                "category:add_torrent",
            ),
        ]);
        keyboard_rows.push(vec![InlineKeyboardButton::callback(
            translate(user, "⏯ Pause/Resume", None),
            "menu_pause_resume:",
        )]);
    }

    if role == "administrator" {
        keyboard_rows.push(vec![InlineKeyboardButton::callback(
            translate(user, "🗑 Delete", None),
            "menu_delete:",
        )]);
        keyboard_rows.push(vec![InlineKeyboardButton::callback(
            translate(user, "📂 Categories", None),
            "menu_categories:",
        )]);
        keyboard_rows.push(vec![InlineKeyboardButton::callback(
            translate(user, "⚙️ Settings", None),
            "settings:",
        )]);
    }

    // Reset current action
    redis
        .set(&format!("action:{}", user.user_id), "", None)
        .await;

    let text = translate(user, "Welcome to QBittorrent Bot", None);
    let keyboard = InlineKeyboardMarkup::new(keyboard_rows);

    if let Some(mid) = message_id {
        let _ = bot
            .edit_message_text(chat_id, mid, &text)
            .reply_markup(keyboard)
            .await;
    } else {
        let _ = bot
            .send_message(chat_id, &text)
            .reply_markup(keyboard)
            .await;
    }

    Ok(())
}

pub async fn list_active_torrents(
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

    let torrents = manager
        .get_torrents(None, clean_filter)
        .await
        .unwrap_or_default();

    let mut keyboard_rows = Vec::new();

    let dl_mark = if clean_filter == Some("downloading") {
        "*"
    } else {
        ""
    };
    let comp_mark = if clean_filter == Some("completed") {
        "*"
    } else {
        ""
    };
    let pause_mark = if clean_filter == Some("paused") {
        "*"
    } else {
        ""
    };

    let mut vars_dl = HashMap::new();
    vars_dl.insert("active".to_string(), dl_mark.to_string());
    let mut vars_comp = HashMap::new();
    vars_comp.insert("active".to_string(), comp_mark.to_string());
    let mut vars_pause = HashMap::new();
    vars_pause.insert("active".to_string(), pause_mark.to_string());

    keyboard_rows.push(vec![
        InlineKeyboardButton::callback(
            translate(user, "⏳ {active} Downloading", Some(&vars_dl)),
            "by_status_list:downloading",
        ),
        InlineKeyboardButton::callback(
            translate(user, "✔️ {active} Completed", Some(&vars_comp)),
            "by_status_list:completed",
        ),
        InlineKeyboardButton::callback(
            translate(user, "⏸️ {active} Paused", Some(&vars_pause)),
            "by_status_list:paused",
        ),
    ]);

    let text_no_torrents = translate(user, "There are no torrents", None);
    let text_back_menu = translate(user, "🔙 Menu", None);

    if torrents.is_empty() {
        keyboard_rows.push(vec![InlineKeyboardButton::callback(
            text_back_menu,
            "menu:",
        )]);
        let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
        let _ = bot
            .edit_message_text(chat_id, message_id, text_no_torrents)
            .reply_markup(keyboard)
            .await;
        return Ok(());
    }

    for t in torrents {
        let btn_text = if t.name.len() > 40 {
            format!("{}...", &t.name[..37])
        } else {
            t.name.clone()
        };
        let cb_data = match callback_prefix {
            Some(prefix) => format!("{}:{}", prefix, t.hash),
            None => format!("torrentInfo:{}", t.hash),
        };
        keyboard_rows.push(vec![InlineKeyboardButton::callback(btn_text, cb_data)]);
    }

    keyboard_rows.push(vec![InlineKeyboardButton::callback(
        text_back_menu,
        "menu:",
    )]);
    let keyboard = InlineKeyboardMarkup::new(keyboard_rows);

    let _ = bot
        .edit_message_reply_markup(chat_id, message_id)
        .reply_markup(keyboard)
        .await;
    Ok(())
}

pub async fn list_categories(
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
        keyboard_rows.push(vec![InlineKeyboardButton::callback(
            text_back_menu,
            "menu:",
        )]);
        let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
        let text_no_categories = translate(user, "There are no categories", None);
        let _ = bot
            .edit_message_text(chat_id, message_id, text_no_categories)
            .reply_markup(keyboard)
            .await;
        return Ok(());
    }

    for cat in categories.unwrap() {
        keyboard_rows.push(vec![InlineKeyboardButton::callback(
            &cat,
            format!("{}:{}", callback_prefix, cat),
        )]);
    }

    keyboard_rows.push(vec![InlineKeyboardButton::callback(
        text_back_menu,
        "menu:",
    )]);
    let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
    let text_choose = translate(user, "Choose a category:", None);
    let _ = bot
        .edit_message_text(chat_id, message_id, text_choose)
        .reply_markup(keyboard)
        .await;

    Ok(())
}

pub async fn render_client_settings(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    user: &UserSettings,
    manager: &dyn TorrentClient,
    settings: &Settings,
) -> Result<(), String> {
    let speed_limit = manager.get_speed_limit_mode().await.unwrap_or(false);
    let speed_limit_status = if speed_limit {
        translate(user, "✅ Enabled", None)
    } else {
        translate(user, "❌ Disabled", None)
    };

    let mut vars = HashMap::new();
    vars.insert("speed_limit_status".to_string(), speed_limit_status);
    let confs = translate(user, "**Speed Limit**: {speed_limit_status}", Some(&vars));

    let client_type = {
        let mut chars = settings.client.r#type.chars();
        match chars.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        }
    };

    let mut vars_text = HashMap::new();
    vars_text.insert("client_type".to_string(), client_type);
    vars_text.insert("configs".to_string(), confs);
    let text = translate(
        user,
        "Edit {client_type} client settings \n\n{configs}",
        Some(&vars_text),
    );

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            translate(user, "🐢 Toggle Speed Limit", None),
            "toggle_speed_limit:",
        )],
        vec![InlineKeyboardButton::callback(
            translate(user, "✅ Check Client connection", None),
            "check_connection:",
        )],
        vec![InlineKeyboardButton::callback(
            translate(user, "🔙 Settings", None),
            "settings:",
        )],
    ]);

    let _ = bot
        .edit_message_text(chat_id, message_id, text)
        .reply_markup(keyboard)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await;
    Ok(())
}

pub async fn render_torrent_details(
    bot: &Bot,
    chat_id: ChatId,
    message_id: MessageId,
    user: &UserSettings,
    torrent: &Torrent,
) -> Result<(), String> {
    let mut text = format!("{}\n", escape_markdown(&torrent.name));

    if torrent.progress >= 1.0 {
        text.push_str(&t(
            "**COMPLETED**\n",
            user.locale.as_deref().unwrap_or("en"),
            None,
        ));
    } else {
        text.push_str(&format_progress(torrent.progress, 20));
    }

    if !torrent.state.contains("stalled") {
        let current_state = {
            let mut chars = torrent.state.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        };
        let speed = convert_size(torrent.dlspeed);
        let mut vars_state = HashMap::new();
        vars_state.insert("current_state".to_string(), current_state);
        vars_state.insert("download_speed".to_string(), speed);
        text.push_str(&translate(
            user,
            "**State:** {current_state} \n**Download Speed:** {download_speed}/s\n",
            Some(&vars_state),
        ));
    }

    let mut vars_size = HashMap::new();
    vars_size.insert("torrent_size".to_string(), convert_size(torrent.size));
    text.push_str(&translate(
        user,
        "**Size:** {torrent_size}\n",
        Some(&vars_size),
    ));

    if !torrent.state.contains("stalled") {
        let mut vars_eta = HashMap::new();
        vars_eta.insert("torrent_eta".to_string(), convert_eta(torrent.eta));
        text.push_str(&translate(
            user,
            "**ETA:** {torrent_eta}\n",
            Some(&vars_eta),
        ));
    }

    if let Some(ref cat) = torrent.category {
        let mut vars_cat = HashMap::new();
        vars_cat.insert("torrent_category".to_string(), cat.clone());
        text.push_str(&translate(
            user,
            "**Category:** {torrent_category}\n",
            Some(&vars_cat),
        ));
    }

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback(
            translate(user, "💾 Export torrent", None),
            format!("export:{}", torrent.hash),
        )],
        vec![InlineKeyboardButton::callback(
            translate(user, "📝 Edit Category", None),
            format!("edit_torrent_cat:{}", torrent.hash),
        )],
        vec![InlineKeyboardButton::callback(
            translate(user, "⏸ Pause", None),
            format!("pause:{}", torrent.hash),
        )],
        vec![InlineKeyboardButton::callback(
            translate(user, "▶️ Resume", None),
            format!("resume:{}", torrent.hash),
        )],
        vec![InlineKeyboardButton::callback(
            translate(user, "🗑 Delete", None),
            format!("delete_one:{}", torrent.hash),
        )],
        vec![InlineKeyboardButton::callback(
            translate(user, "🔙 Menu", None),
            "menu:",
        )],
    ]);

    let _ = bot
        .edit_message_text(chat_id, message_id, text)
        .reply_markup(keyboard)
        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
        .await;
    Ok(())
}

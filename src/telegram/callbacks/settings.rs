use std::collections::BTreeMap as HashMap;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ChatId};
use teloxide::Bot;

use crate::telegram::menu::render_client_settings;
use crate::telegram::state::{translate, BotState};
use crate::config::{UserSettings, Settings};

pub async fn handle_settings_callback(
    bot: &Bot,
    q: &CallbackQuery,
    state: &BotState,
    user: &UserSettings,
    settings: &Settings,
    prefix: &str,
    _arg1: &str,
    _arg2: &str,
    chat_id: ChatId,
    message_id: MessageId,
    is_admin: bool,
) -> ResponseResult<bool> {
    match prefix {
        "settings" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Client Settings", None),
                    "edit_client:",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Reload Settings", None),
                    "reload_settings:",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Menu", None),
                    "menu:",
                )],
            ]);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    translate(user, "QBittorrentBot Settings", None),
                )
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "edit_client" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = render_client_settings(
                bot,
                chat_id,
                message_id,
                user,
                &*state.manager,
                settings,
            )
            .await;
            Ok(true)
        }
        "toggle_speed_limit" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.toggle_speed_limit().await;
            let _ = render_client_settings(
                bot,
                chat_id,
                message_id,
                user,
                &*state.manager,
                settings,
            )
            .await;
            Ok(true)
        }
        "check_connection" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            match state.manager.check_connection().await {
                Ok(version) => {
                    let mut vars = HashMap::new();
                    vars.insert("version".to_string(), version);
                    let text = translate(
                        user,
                        "? The connection works. QBittorrent version: {version}",
                        Some(&vars),
                    );
                    let _ = bot
                        .answer_callback_query(q.id.clone())
                        .text(text)
                        .show_alert(true)
                        .await;
                }
                Err(_) => {
                    let _ = bot
                        .answer_callback_query(q.id.clone())
                        .text(translate(
                            user,
                            "? Unable to establish connection with QBittorrent",
                            None,
                        ))
                        .show_alert(true)
                        .await;
                }
            }
            Ok(true)
        }
        "reload_settings" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let reloaded = Settings::load_settings();
            {
                let mut settings_write = state.settings.write().await;
                *settings_write = reloaded;
            }
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "? Settings Reloaded", None))
                .show_alert(true)
                .await;
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn unauthorized_alert(bot: &Bot, q: &CallbackQuery, user: &UserSettings) -> ResponseResult<()> {
    let _ = bot
        .answer_callback_query(q.id.clone())
        .text(translate(user, "You are not authorized to use this bot", None))
        .show_alert(true)
        .await;
    Ok(())
}

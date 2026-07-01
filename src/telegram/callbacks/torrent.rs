use std::collections::BTreeMap as HashMap;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId, ChatId};
use teloxide::Bot;

use crate::telegram::menu::{list_active_torrents, render_torrent_details, send_menu};
use crate::telegram::state::{translate, BotState};
use crate::config::UserSettings;

pub async fn handle_torrent_callback(
    bot: &Bot,
    q: &CallbackQuery,
    state: &BotState,
    user: &UserSettings,
    prefix: &str,
    arg1: &str,
    _arg2: &str,
    chat_id: ChatId,
    message_id: MessageId,
    is_manager: bool,
    is_admin: bool,
) -> ResponseResult<bool> {
    match prefix {
        "menu_pause_resume" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![
                    InlineKeyboardButton::callback(translate(user, "? Pause", None), "pause:"),
                    InlineKeyboardButton::callback(translate(user, "?? Resume", None), "resume:"),
                ],
                vec![
                    InlineKeyboardButton::callback(
                        translate(user, "? Pause All", None),
                        "pause_all:",
                    ),
                    InlineKeyboardButton::callback(
                        translate(user, "?? Resume All", None),
                        "resume_all:",
                    ),
                ],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Menu", None),
                    "menu:",
                )],
            ]);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    translate(user, "Pause/Resume a torrent", None),
                )
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "pause_all" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.pause_all().await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Paused all torrents", None))
                .await;
            Ok(true)
        }
        "resume_all" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.resume_all().await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Resumed all torrents", None))
                .await;
            Ok(true)
        }
        "pause" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            if arg1.is_empty() {
                let _ = list_active_torrents(
                    bot,
                    chat_id,
                    message_id,
                    user,
                    &*state.manager,
                    Some("pause"),
                    None,
                )
                .await;
            } else {
                let _ = state.manager.pause(arg1).await;
                let _ = bot
                    .answer_callback_query(q.id.clone())
                    .text(translate(user, "Torrent Paused", None))
                    .await;
                if let Ok(Some(t)) = state.manager.get_torrent(arg1, None).await {
                    let _ = render_torrent_details(bot, chat_id, message_id, user, &t).await;
                }
            }
            Ok(true)
        }
        "resume" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            if arg1.is_empty() {
                let _ = list_active_torrents(
                    bot,
                    chat_id,
                    message_id,
                    user,
                    &*state.manager,
                    Some("resume"),
                    None,
                )
                .await;
            } else {
                let _ = state.manager.resume(arg1).await;
                let _ = bot
                    .answer_callback_query(q.id.clone())
                    .text(translate(user, "Torrent Resumed", None))
                    .await;
                if let Ok(Some(t)) = state.manager.get_torrent(arg1, None).await {
                    let _ = render_torrent_details(bot, chat_id, message_id, user, &t).await;
                }
            }
            Ok(true)
        }
        "menu_delete" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Delete", None),
                    "delete_one:",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Delete All", None),
                    "delete_all:",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Menu", None),
                    "menu:",
                )],
            ]);
            let _ = bot
                .edit_message_text(chat_id, message_id, "Delete a torrent")
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "delete_one" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            if arg1.is_empty() {
                let _ = list_active_torrents(
                    bot,
                    chat_id,
                    message_id,
                    user,
                    &*state.manager,
                    Some("delete_one"),
                    None,
                )
                .await;
            } else {
                let keyboard = InlineKeyboardMarkup::new(vec![
                    vec![InlineKeyboardButton::callback(
                        translate(user, "?? Delete torrent", None),
                        format!("delete_one_no_data:{}", arg1),
                    )],
                    vec![InlineKeyboardButton::callback(
                        translate(user, "?? Delete torrent and data", None),
                        format!("delete_one_data:{}", arg1),
                    )],
                    vec![InlineKeyboardButton::callback(
                        translate(user, "?? Menu", None),
                        "menu:",
                    )],
                ]);
                let _ = bot
                    .edit_message_reply_markup(chat_id, message_id)
                    .reply_markup(keyboard)
                    .await;
            }
            Ok(true)
        }
        "delete_one_no_data" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.delete_one_no_data(arg1).await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Torrent deleted", None))
                .await;
            let _ = send_menu(bot, chat_id, Some(message_id), user, &state.redis).await;
            Ok(true)
        }
        "delete_one_data" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.delete_one_data(arg1).await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Torrent and data deleted", None))
                .await;
            let _ = send_menu(bot, chat_id, Some(message_id), user, &state.redis).await;
            Ok(true)
        }
        "delete_all" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Delete all torrents", None),
                    "delete_all_no_data:",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Delete all torrents and data", None),
                    "delete_all_data:",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Menu", None),
                    "menu:",
                )],
            ]);
            let _ = bot
                .edit_message_reply_markup(chat_id, message_id)
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "delete_all_no_data" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.delete_all_no_data().await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Deleted all torrents", None))
                .await;
            let _ = send_menu(bot, chat_id, Some(message_id), user, &state.redis).await;
            Ok(true)
        }
        "delete_all_data" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.delete_all_data().await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Deleted all torrents and data", None))
                .await;
            let _ = send_menu(bot, chat_id, Some(message_id), user, &state.redis).await;
            Ok(true)
        }
        "torrentInfo" => {
            if let Ok(Some(t)) = state.manager.get_torrent(arg1, None).await {
                let _ = render_torrent_details(bot, chat_id, message_id, user, &t).await;
            } else {
                let _ = bot
                    .answer_callback_query(q.id.clone())
                    .text("Torrent not found")
                    .show_alert(true)
                    .await;
            }
            Ok(true)
        }
        "export" => {
            match state.manager.export_torrent(arg1).await {
                Ok((bytes, filename)) => {
                    let _ = bot
                        .send_document(chat_id, InputFile::memory(bytes).file_name(filename))
                        .await;
                    let _ = bot.answer_callback_query(q.id.clone()).await;
                }
                Err(_) => {
                    let _ = bot
                        .answer_callback_query(q.id.clone())
                        .text("Failed to export torrent")
                        .show_alert(true)
                        .await;
                }
            }
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

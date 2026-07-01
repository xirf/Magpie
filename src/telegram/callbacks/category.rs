use std::collections::BTreeMap as HashMap;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ChatId};
use teloxide::Bot;

use crate::telegram::menu::{list_categories, send_menu};
use crate::telegram::state::{translate, BotState};
use crate::config::UserSettings;

pub async fn handle_category_callback(
    bot: &Bot,
    q: &CallbackQuery,
    state: &BotState,
    user: &UserSettings,
    prefix: &str,
    arg1: &str,
    arg2: &str,
    chat_id: ChatId,
    message_id: MessageId,
    is_manager: bool,
    is_admin: bool,
) -> ResponseResult<bool> {
    let user_id = q.from.id.0 as i64;
    match prefix {
        "category" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let action = arg1;
            let categories = state.manager.get_categories().await.unwrap_or(None);

            let cb_prefix = if action == "add_magnet" {
                "add_magnet"
            } else {
                "add_torrent"
            };
            let mut keyboard_rows = Vec::new();
            if let Some(cats) = categories {
                for cat in cats {
                    keyboard_rows.push(vec![InlineKeyboardButton::callback(
                        &cat,
                        format!("{}:{}", cb_prefix, cat),
                    )]);
                }
            }
            keyboard_rows.push(vec![InlineKeyboardButton::callback(
                "None",
                format!("{}:None", cb_prefix),
            )]);
            keyboard_rows.push(vec![InlineKeyboardButton::callback(
                translate(user, "?? Menu", None),
                "menu:",
            )]);

            let keyboard = InlineKeyboardMarkup::new(keyboard_rows);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    translate(user, "Choose a category:", None),
                )
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "add_magnet" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            state
                .redis
                .set(
                    &format!("action:{}", user_id),
                    &format!("magnet#{}", arg1),
                    None,
                )
                .await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Send a magnet link", None))
                .await;

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                translate(user, "?? Menu", None),
                "menu:",
            )]]);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    translate(user, "Send a magnet link", None),
                )
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "add_torrent" => {
            if !is_manager {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            state
                .redis
                .set(
                    &format!("action:{}", user_id),
                    &format!("torrent#{}", arg1),
                    None,
                )
                .await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Send a torrent file", None))
                .await;

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                translate(user, "?? Menu", None),
                "menu:",
            )]]);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    translate(user, "Send a torrent file", None),
                )
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "menu_categories" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let keyboard = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    translate(user, "? Add Category", None),
                    "add_category:",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Remove Category", None),
                    "select_category:remove_category",
                )],
                vec![InlineKeyboardButton::callback(
                    translate(user, "?? Modify Category", None),
                    "select_category:modify_category",
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
                    translate(user, "Pause/Resume a download", None),
                )
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "add_category" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            state
                .redis
                .set(&format!("action:{}", user_id), "category_name", None)
                .await;
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(translate(user, "Send the category name", None))
                .await;

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                translate(user, "?? Menu", None),
                "menu:",
            )]]);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    translate(user, "Send the category name", None),
                )
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "select_category" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = list_categories(bot, chat_id, message_id, user, &*state.manager, arg1).await;
            Ok(true)
        }
        "remove_category" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            let _ = state.manager.remove_category(arg1).await;
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                translate(user, "?? Menu", None),
                "menu:",
            )]]);
            let mut vars = HashMap::new();
            vars.insert("category_name".to_string(), arg1.to_string());
            let text = translate(
                user,
                "The category {category_name} has been removed",
                Some(&vars),
            );
            let _ = bot
                .edit_message_text(chat_id, message_id, text)
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "modify_category" => {
            if !is_admin {
                unauthorized_alert(bot, q, user).await?;
                return Ok(true);
            }
            state
                .redis
                .set(
                    &format!("action:{}", user_id),
                    &format!("category_dir_modify#{}", arg1),
                    None,
                )
                .await;
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                translate(user, "?? Menu", None),
                "menu:",
            )]]);
            let mut vars = HashMap::new();
            vars.insert("category_name".to_string(), arg1.to_string());
            let text = translate(
                user,
                "Send new path for category {category_name}",
                Some(&vars),
            );
            let _ = bot
                .edit_message_text(chat_id, message_id, text)
                .reply_markup(keyboard)
                .await;
            Ok(true)
        }
        "edit_torrent_cat" => {
            let _ = list_categories(
                bot,
                chat_id,
                message_id,
                user,
                &*state.manager,
                &format!("torrent_cat:{}", arg1),
            )
            .await;
            Ok(true)
        }
        "torrent_cat" => {
            let _ = state.manager.set_torrents_category(arg2, arg1).await;
            let mut vars = HashMap::new();
            vars.insert("category".to_string(), arg2.to_string());
            let text = translate(user, "Torrent category changed to {category}", Some(&vars));
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(text)
                .show_alert(true)
                .await;
            let _ = send_menu(bot, chat_id, Some(message_id), user, &state.redis).await;
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

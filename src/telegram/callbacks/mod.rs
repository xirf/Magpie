use teloxide::prelude::*;
use teloxide::Bot;

pub mod torrent;
pub mod category;
pub mod settings;

use super::menu::{list_active_torrents, send_menu};
use super::state::{get_resolved_user, BotState};

pub async fn handle_callback_query(
    bot: Bot,
    q: CallbackQuery,
    state: BotState,
) -> ResponseResult<()> {
    let user_id = q.from.id.0 as i64;
    let current_settings = state.settings.read().await.clone();
    let user = get_resolved_user(&current_settings, user_id);

    let message = match q.message.as_ref() {
        Some(m) => m,
        None => return Ok(()),
    };
    let chat_id = message.chat.id;
    let message_id = message.id;

    let data = match q.data.as_ref() {
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
            return Ok(());
        }
        "list" => {
            let _ = list_active_torrents(
                &bot,
                chat_id,
                message_id,
                &user,
                &*state.manager,
                None,
                None,
            )
            .await;
            return Ok(());
        }
        "by_status_list" => {
            let status = if arg1.is_empty() { None } else { Some(arg1) };
            let _ = list_active_torrents(
                &bot,
                chat_id,
                message_id,
                &user,
                &*state.manager,
                None,
                status,
            )
            .await;
            return Ok(());
        }
        _ => {}
    }

    // Delegate to torrent callbacks
    let handled = torrent::handle_torrent_callback(
        &bot,
        &q,
        &state,
        &user,
        prefix,
        arg1,
        arg2,
        chat_id,
        message_id,
        is_manager,
        is_admin,
    )
    .await?;

    if handled {
        return Ok(());
    }

    // Delegate to category callbacks
    let handled = category::handle_category_callback(
        &bot,
        &q,
        &state,
        &user,
        prefix,
        arg1,
        arg2,
        chat_id,
        message_id,
        is_manager,
        is_admin,
    )
    .await?;

    if handled {
        return Ok(());
    }

    // Delegate to settings callbacks
    let handled = settings::handle_settings_callback(
        &bot,
        &q,
        &state,
        &user,
        &current_settings,
        prefix,
        arg1,
        arg2,
        chat_id,
        message_id,
        is_admin,
    )
    .await?;

    if handled {
        return Ok(());
    }

    let _ = bot.answer_callback_query(q.id).await;
    Ok(())
}

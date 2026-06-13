use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use teloxide::prelude::*;
use teloxide::types::InlineKeyboardMarkup;
use teloxide::Bot;

use crate::config::{Settings, UserSettings};
use crate::torrent_client::TorrentClient;
use crate::redis_client::RedisWrapper;
use crate::i18n::t;

#[derive(Clone)]
pub struct BotState {
    pub settings: Arc<RwLock<Settings>>,
    pub manager: Arc<dyn TorrentClient>,
    pub redis: RedisWrapper,
}

pub(crate) fn get_resolved_user(settings: &Settings, telegram_id: i64) -> UserSettings {
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

pub(crate) fn translate(user: &UserSettings, key: &str, vars: Option<&HashMap<String, String>>) -> String {
    let locale = user.locale.as_deref().unwrap_or("en");
    t(key, locale, vars)
}

pub(crate) async fn get_user_and_check_auth(bot: &Bot, settings: &Settings, msg: &Message) -> Option<UserSettings> {
    let telegram_id = msg.from()?.id.0 as i64;
    let user = get_resolved_user(settings, telegram_id);

    // If user is not authorized (not in the list and no default/wildcard user exists)
    let is_authorized = settings.users.iter().any(|u| u.user_id == telegram_id || u.user_id == 0);
    if !is_authorized {
        let keyboard = InlineKeyboardMarkup::new(vec![vec![
            teloxide::types::InlineKeyboardButton::url("Github", "https://github.com/ch3p4ll3/QBittorrentBot/".parse().unwrap())
        ]]);
        let txt = t("You are not authorized to use this bot", user.locale.as_deref().unwrap_or("en"), None);
        let _ = bot.send_message(msg.chat.id, txt).reply_markup(keyboard).await;
        return None;
    }

    Some(user)
}

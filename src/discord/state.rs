use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::{Settings, UserSettings};
use crate::torrent_client::TorrentClient;
use crate::i18n::t;

pub struct Handler {
    pub settings: Arc<RwLock<Settings>>,
    pub manager: Arc<dyn TorrentClient>,
}

pub(crate) fn translate(user: &UserSettings, key: &str, vars: Option<&HashMap<String, String>>) -> String {
    let locale = user.locale.as_deref().unwrap_or("en");
    t(key, locale, vars)
}

pub(crate) fn get_resolved_user(settings: &Settings, discord_id: &str) -> UserSettings {
    settings.users.iter()
        .find(|u| u.discord_id.as_deref() == Some(discord_id))
        .cloned()
        .or_else(|| {
            settings.users.iter()
                .find(|u| u.discord_id.is_none())
                .cloned()
        })
        .unwrap_or_else(|| {
            UserSettings {
                user_id: 0,
                discord_id: Some(discord_id.to_string()),
                role: "reader".to_string(),
                locale: Some("en".to_string()),
                notify: true,
                notification_filter: vec![],
            }
        })
}

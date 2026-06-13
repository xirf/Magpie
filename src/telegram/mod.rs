pub mod state;
pub mod menu;
pub mod callbacks;
pub mod messages;

use std::sync::Arc;
use tokio::sync::RwLock;
use teloxide::prelude::*;
use teloxide::Bot;

use crate::config::Settings;
use crate::torrent_client::TorrentClient;
use crate::redis_client::RedisWrapper;

pub use state::BotState;

pub async fn start_telegram_bot(
    settings: Arc<RwLock<Settings>>,
    redis: RedisWrapper,
    manager: Arc<dyn TorrentClient>,
) -> teloxide::Bot {
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
        .branch(Update::filter_callback_query().endpoint(callbacks::handle_callback_query))
        .branch(Update::filter_message().endpoint(messages::handle_message));

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

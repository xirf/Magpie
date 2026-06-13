mod config;
mod i18n;
mod db;
mod redis_client;
mod s3;
mod qbittorrent;
mod transmission;
mod torrent_client;
mod discord;
mod telegram;
mod tasks;
mod utils;
mod server;

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{self, Duration};

use crate::config::Settings;
use crate::redis_client::RedisWrapper;

#[tokio::main]
async fn main() {
    println!("Starting Magpie (Rust version)...");

    // Load configuration settings
    let mut settings = Settings::load_settings();

    // Sync users with SQLite database
    match crate::db::sync_users(&settings.users) {
        Ok(synced_users) => {
            settings.users = synced_users;
        }
        Err(e) => {
            eprintln!("Failed to sync users with SQLite database, falling back to config users: {:?}", e);
        }
    }

    let settings_arc = Arc::new(RwLock::new(settings));

    // Create and connect to Redis client
    let redis = RedisWrapper::new();
    let redis_url = {
        let s = settings_arc.read().await;
        s.redis.url.clone()
    };
    redis.connect(redis_url.as_deref()).await;

    // Check active bot providers
    let (is_tg_active, is_dc_active) = {
        let s = settings_arc.read().await;
        
        let tg = s.telegram.enabled 
            && !s.telegram.bot_token.is_empty() 
            && s.telegram.bot_token != "PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE";
            
        let dc = s.discord.enabled 
            && s.discord.token.is_some() 
            && s.discord.token.as_deref().unwrap_or("") != "PUT_YOUR_DISCORD_BOT_TOKEN_HERE";
            
        (tg, dc)
    };

    if !is_tg_active && !is_dc_active {
        eprintln!("\n❌ ERROR: No active bot providers configured!");
        eprintln!("Please configure a valid Telegram or Discord bot token in data/config.yml.");
        eprintln!("Make sure to set enabled: true for the provider you want to use.\n");
        std::process::exit(1);
    }

    // Initialize shared torrent client manager (qBittorrent or Transmission)
    let torrent_client = {
        let s = settings_arc.read().await;
        crate::torrent_client::create_client(&s)
    };

    // Initialize Telegram bot
    let mut tg_bot = None;
    if is_tg_active {
        println!("Starting Telegram Bot...");
        let bot = telegram::start_telegram_bot(settings_arc.clone(), redis.clone(), torrent_client.clone()).await;
        tg_bot = Some(bot);
    } else {
        println!("Telegram bot is disabled or not configured.");
    }

    // Initialize Discord bot
    let mut dc_client = None;
    if is_dc_active {
        println!("Starting Discord Bot...");
        match discord::start_discord_bot(settings_arc.clone(), torrent_client.clone()).await {
            Ok(client) => {
                dc_client = Some(client);
            }
            Err(e) => {
                eprintln!("Failed to initialize Discord bot: {:?}", e);
            }
        }
    } else {
        println!("Discord bot is disabled or not configured.");
    }

    // Extract Discord HTTP ref if active
    let dc_http = dc_client.as_ref().map(|c| c.http.clone());

    // Spawn Discord bot if active
    if let Some(mut client) = dc_client {
        tokio::spawn(async move {
            if let Err(e) = client.start().await {
                eprintln!("Discord bot error: {:?}", e);
            }
        });
    }

    // Spin up local download web server if enabled
    let settings_for_server = settings_arc.clone();
    tokio::spawn(async move {
        let enabled = {
            let s = settings_for_server.read().await;
            s.local_server.enabled
        };
        if enabled {
            server::start_server(settings_for_server).await;
        }
    });

    // Watch config.yml for hot reloading
    tasks::watch_config(settings_arc.clone());

    // Schedule periodic completed torrent checks (every 60 seconds)
    let tg_bot_for_check = tg_bot.clone();
    let redis_for_check = redis.clone();
    let settings_for_check = settings_arc.clone();
    let client_for_check = torrent_client.clone();
    let dc_http_for_check = dc_http.clone();

    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let _ = crate::db::prune_expired_local_downloads();
            let current_settings = settings_for_check.read().await.clone();
            tasks::torrent_finished(
                tg_bot_for_check.as_ref(),
                dc_http_for_check.clone(),
                &redis_for_check,
                &current_settings,
                &*client_for_check,
            ).await;
        }
    });

    // Schedule periodic progress update edits (every 1 hour)
    let tg_bot_for_progress = tg_bot.clone();
    let redis_for_progress = redis.clone();
    let settings_for_progress = settings_arc.clone();
    let client_for_progress = torrent_client.clone();
    let dc_http_for_progress = dc_http.clone();

    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            let current_settings = settings_for_progress.read().await.clone();
            tasks::torrent_progress_update(
                tg_bot_for_progress.as_ref(),
                dc_http_for_progress.clone(),
                &redis_for_progress,
                &current_settings,
                &*client_for_progress,
            ).await;
        }
    });

    println!("Magpie is online and polling for updates...");

    // Setup graceful shutdown handler
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            println!("Stopping bot and cleaning up...");
        }
    }
}

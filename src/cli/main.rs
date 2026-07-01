//! magpie-cli -- Command-line interface for controlling the configured torrent client.
//!
//! Reads the same data/config.yml as the bot daemon.
//! No Telegram or Discord token is required.
//!
//! Output is newline-delimited JSON -- easy to pipe into jq, scripts, or tests.
//!
//! Usage examples:
//!   magpie-cli ping
//!   magpie-cli add "magnet:?xt=urn:btih:..."
//!   magpie-cli add "magnet:..." --category movies
//!   magpie-cli add ./ubuntu.torrent
//!   magpie-cli list
//!   magpie-cli list --status downloading
//!   magpie-cli info <hash>
//!   magpie-cli pause <hash>
//!   magpie-cli resume <hash>
//!   magpie-cli delete <hash>
//!   magpie-cli delete <hash> --with-data
//!   magpie-cli pause-all
//!   magpie-cli resume-all
//!   magpie-cli speed-limit
//!   magpie-cli speed-limit-status

use clap::{Parser, Subcommand};
use magpie::config::Settings;
use magpie::torrent_client::create_client;
use std::process;

#[derive(Parser)]
#[command(
    name = "magpie-cli",
    about = "Control your torrent client from the command line (uses data/config.yml)",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Check connection to the configured torrent client
    Ping,

    /// Add a magnet link, HTTP(S) URL, or local .torrent file path
    Add {
        /// Magnet URI, HTTP URL, or path to a local .torrent file
        input: String,
        /// Assign the torrent to a category
        #[arg(short, long)]
        category: Option<String>,
    },

    /// List all torrents (optionally filtered by status)
    List {
        /// Status filter: downloading, completed, paused, error, queued
        #[arg(short, long)]
        status: Option<String>,
    },

    /// Show detailed info for a single torrent
    Info {
        /// Torrent info-hash
        hash: String,
    },

    /// Pause a torrent
    Pause {
        /// Torrent info-hash
        hash: String,
    },

    /// Resume a paused torrent
    Resume {
        /// Torrent info-hash
        hash: String,
    },

    /// Delete a torrent (keeps downloaded data by default)
    Delete {
        /// Torrent info-hash
        hash: String,
        /// Also delete the downloaded data from disk
        #[arg(long)]
        with_data: bool,
    },

    /// Pause all torrents
    PauseAll,

    /// Resume all torrents
    ResumeAll,

    /// Toggle alternate speed limit mode
    SpeedLimit,

    /// Show current speed limit mode (on/off)
    SpeedLimitStatus,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let settings = Settings::load_settings();
    let client = create_client(&settings);

    let result: serde_json::Value = match cli.command {
        Cmd::Ping => match client.check_connection().await {
            Ok(v) => serde_json::json!({ "ok": true, "version": v }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::Add { input, category } => {
            let cat = category.as_deref();
            let lower = input.to_lowercase();

            let res = if lower.starts_with("magnet:?xt=urn:") {
                client.add_magnet(&input, cat).await
            } else if lower.starts_with("http://") || lower.starts_with("https://") {
                match client.add_url(&input, cat).await {
                    Ok(true) => Ok(true),
                    Err(e) if e.contains("not supported") => {
                        match reqwest::get(&input).await {
                            Ok(resp) => match resp.bytes().await {
                                Ok(bytes) => {
                                    let filename = input
                                        .split('/')
                                        .next_back()
                                        .filter(|f| f.ends_with(".torrent"))
                                        .unwrap_or("downloaded.torrent");
                                    client.add_torrent(bytes.to_vec(), filename, cat).await
                                }
                                Err(err) => Err(err.to_string()),
                            },
                            Err(err) => Err(err.to_string()),
                        }
                    }
                    res => res,
                }
            } else {
                // Treat as local file path
                match std::fs::read(&input) {
                    Ok(bytes) => {
                        let filename = std::path::Path::new(&input)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("file.torrent");
                        client.add_torrent(bytes, filename, cat).await
                    }
                    Err(e) => Err(format!("Cannot read file {:?}: {}", input, e)),
                }
            };

            match res {
                Ok(true) => serde_json::json!({ "ok": true }),
                Ok(false) => serde_json::json!({
                    "ok": false,
                    "error": "Client returned false -- torrent may already exist"
                }),
                Err(e) => serde_json::json!({ "ok": false, "error": e }),
            }
        }

        Cmd::List { status } => match client.get_torrents(None, status.as_deref()).await {
            Ok(torrents) => {
                let arr: Vec<serde_json::Value> = torrents
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "hash": t.hash,
                            "name": t.name,
                            "state": t.state,
                            "progress_pct": (t.progress * 100.0).round() as u32,
                            "size_bytes": t.size,
                            "dlspeed_bps": t.dlspeed,
                            "eta_secs": t.eta,
                            "category": t.category,
                        })
                    })
                    .collect();
                serde_json::json!({ "ok": true, "count": arr.len(), "torrents": arr })
            }
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::Info { hash } => match client.get_torrent(&hash, None).await {
            Ok(Some(t)) => serde_json::json!({
                "ok": true,
                "hash": t.hash,
                "name": t.name,
                "state": t.state,
                "progress_pct": (t.progress * 100.0).round() as u32,
                "size_bytes": t.size,
                "dlspeed_bps": t.dlspeed,
                "eta_secs": t.eta,
                "category": t.category,
                "save_path": t.save_path,
                "content_path": t.content_path,
                "num_seeds": t.num_seeds,
                "num_peers": t.num_peers,
            }),
            Ok(None) => serde_json::json!({ "ok": false, "error": "torrent not found" }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::Pause { hash } => match client.pause(&hash).await {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::Resume { hash } => match client.resume(&hash).await {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::Delete { hash, with_data } => {
            let res = if with_data {
                client.delete_one_data(&hash).await
            } else {
                client.delete_one_no_data(&hash).await
            };
            match res {
                Ok(()) => serde_json::json!({ "ok": true }),
                Err(e) => serde_json::json!({ "ok": false, "error": e }),
            }
        }

        Cmd::PauseAll => match client.pause_all().await {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::ResumeAll => match client.resume_all().await {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::SpeedLimit => match client.toggle_speed_limit().await {
            Ok(active) => serde_json::json!({ "ok": true, "speed_limit_active": active }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },

        Cmd::SpeedLimitStatus => match client.get_speed_limit_mode().await {
            Ok(active) => serde_json::json!({ "ok": true, "speed_limit_active": active }),
            Err(e) => serde_json::json!({ "ok": false, "error": e }),
        },
    };

    let exit_ok = result["ok"].as_bool().unwrap_or(false);
    println!("{}", serde_json::to_string_pretty(&result).unwrap());
    if !exit_ok {
        process::exit(1);
    }
}

use magpie::torrent_client::TorrentClient as _;
use reqwest;
use std::env;
use std::process::Command;
use std::sync::Arc;
use serde_json::Value;

use crate::config::{Settings, UserSettings};
use crate::mock_client::{MockMethod, MockTorrentClient};
use crate::redis_client::RedisWrapper;
use crate::torrent_client::{Torrent, TorrentClient};
use crate::utils::extract_hash_from_magnet;

// -- Helpers -------------------------------------------------------------------

fn get_cli_bin() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop(); // remove test binary name
    if path.ends_with("deps") {
        path.pop();
    }
    #[cfg(target_os = "windows")]
    {
        path.join("magpie-cli.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.join("magpie-cli")
    }
}

/// Sets up a clean environment for a CLI test.
/// Returns the mock file path.
fn setup_cli_test() -> std::path::PathBuf {
    let mock_file = env::temp_dir().join(format!(
        "magpie_cli_mock_{}.json",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    if mock_file.exists() {
        let _ = std::fs::remove_file(&mock_file);
    }
    mock_file
}

fn test_settings() -> Settings {
    let mut s = Settings::get_default_settings();
    s.telegram.enabled = false;
    s.discord.enabled = false;
    s.users = vec![
        UserSettings {
            user_id: 111,
            discord_id: None,
            role: "administrator".to_string(),
            locale: Some("en".to_string()),
            notify: true,
            notification_filter: vec![],
        },
        UserSettings {
            user_id: 222,
            discord_id: None,
            role: "reader".to_string(),
            locale: Some("en".to_string()),
            notify: false,
            notification_filter: vec![],
        },
    ];
    s.seed_after_download = "always".to_string();
    s
}

fn sample_magnet(hash: &str, name: &str) -> String {
    format!("magnet:?xt=urn:btih:{}&dn={}", hash, name)
}

fn isolated_db() {
    let tmp = env::temp_dir().join(format!(
        "magpie_test_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    env::set_var("DATABASE_PATH", tmp.to_str().unwrap());
}

fn completed_torrent(hash: &str, name: &str) -> Torrent {
    Torrent {
        hash: hash.to_string(),
        name: name.to_string(),
        progress: 1.0,
        dlspeed: 0,
        state: "completed".to_string(),
        size: 1024 * 1024 * 500,
        eta: 0,
        category: None,
        save_path: Some("/downloads".to_string()),
        content_path: Some(format!("/downloads/{}", name)),
        num_seeds: Some(3),
        num_peers: Some(0),
    }
}

// -- CLI Subprocess Tests ------------------------------------------------------

#[test]
fn test_cli_ping() {
    let mock_file = setup_cli_test();
    let cli = get_cli_bin();

    let output = Command::new(&cli)
        .arg("ping")
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .expect("failed to execute cli binary");

    assert!(output.status.success());
    let res: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(res["ok"], true);
    assert!(res["version"].as_str().unwrap().contains("MockTorrentClient"));
}

#[test]
fn test_cli_add_and_list() {
    let mock_file = setup_cli_test();
    let cli = get_cli_bin();

    // 1. Add torrent
    let magnet = sample_magnet("aabbccddeeff00112233445566778899aabbccdd", "big_movie");
    let output = Command::new(&cli)
        .args(["add", &magnet, "--category", "movies"])
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let res: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(res["ok"], true);

    // 2. List torrents
    let output_list = Command::new(&cli)
        .arg("list")
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output_list.status.success());
    let res_list: Value = serde_json::from_slice(&output_list.stdout).unwrap();
    assert_eq!(res_list["ok"], true);
    assert_eq!(res_list["count"], 1);

    let torrent = &res_list["torrents"][0];
    assert_eq!(torrent["hash"], "aabbccddeeff00112233445566778899aabbccdd");
    assert_eq!(torrent["category"], "movies");
}

#[test]
fn test_cli_info_and_pause_resume() {
    let mock_file = setup_cli_test();
    let cli = get_cli_bin();
    let hash = "1122334455667788990011223344556677889900";

    // Pre-seed a torrent directly into mock file via TorrentClient trait
    let mock_client = MockTorrentClient::new_with_file(mock_file.to_str().unwrap().to_string());
    mock_client.seed_torrent(Torrent {
        hash: hash.to_string(),
        name: "TestTorrent".to_string(),
        progress: 0.25,
        dlspeed: 1000,
        state: "downloading".to_string(),
        size: 5000,
        eta: 100,
        category: None,
        save_path: None,
        content_path: None,
        num_seeds: None,
        num_peers: None,
    });

    // 1. Info
    let output = Command::new(&cli)
        .args(["info", hash])
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    let res: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(res["ok"], true);
    assert_eq!(res["name"], "TestTorrent");
    assert_eq!(res["progress_pct"], 25);

    // 2. Pause
    let output_pause = Command::new(&cli)
        .args(["pause", hash])
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output_pause.status.success());

    // Verify paused state via info
    let output_info2 = Command::new(&cli)
        .args(["info", hash])
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    let res_info2: Value = serde_json::from_slice(&output_info2.stdout).unwrap();
    assert_eq!(res_info2["state"], "paused");

    // 3. Resume
    let output_resume = Command::new(&cli)
        .args(["resume", hash])
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output_resume.status.success());

    // Verify resumed state
    let output_info3 = Command::new(&cli)
        .args(["info", hash])
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    let res_info3: Value = serde_json::from_slice(&output_info3.stdout).unwrap();
    assert_eq!(res_info3["state"], "downloading");
}

#[test]
fn test_cli_delete() {
    let mock_file = setup_cli_test();
    let cli = get_cli_bin();
    let hash = "9999999999999999999999999999999999999999";

    let mock_client = MockTorrentClient::new_with_file(mock_file.to_str().unwrap().to_string());
    mock_client.seed_torrent(completed_torrent(hash, "Trash"));

    // Delete
    let output = Command::new(&cli)
        .args(["delete", hash])
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    // Verify gone
    let output_list = Command::new(&cli)
        .arg("list")
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    let res_list: Value = serde_json::from_slice(&output_list.stdout).unwrap();
    assert_eq!(res_list["count"], 0);
}

#[test]
fn test_cli_speed_limit() {
    let mock_file = setup_cli_test();
    let cli = get_cli_bin();

    // Toggle
    let output = Command::new(&cli)
        .arg("speed-limit")
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    let res: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(res["speed_limit_active"], true);

    // Status
    let output_status = Command::new(&cli)
        .arg("speed-limit-status")
        .env("MAGPIE_MOCK", "1")
        .env("MAGPIE_MOCK_FILE", mock_file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output_status.status.success());
    let res_status: Value = serde_json::from_slice(&output_status.stdout).unwrap();
    assert_eq!(res_status["speed_limit_active"], true);
}

// -- tasks::torrent_finished tests ---------------------------------------------

#[tokio::test]
async fn test_finished_never_policy_pauses_torrent() {
    isolated_db();
    let client = Arc::new(MockTorrentClient::new());
    let hash = "cccccccccccccccccccccccccccccccccccccccc";
    client.seed_torrent(completed_torrent(hash, "Finished Film"));

    let mut settings = test_settings();
    settings.seed_after_download = "never".to_string();
    settings.users[0].notify = false;

    let redis = RedisWrapper::new();
    redis.connect(None).await;

    crate::tasks::torrent_finished(None, None, &redis, &settings, &*client).await;

    let t = client.get_torrent(hash, None).await.unwrap().unwrap();
    assert_eq!(t.state, "paused");
}

#[tokio::test]
async fn test_finished_always_policy_does_not_pause() {
    isolated_db();
    let client = Arc::new(MockTorrentClient::new());
    let hash = "dddddddddddddddddddddddddddddddddddddddd";
    client.seed_torrent(completed_torrent(hash, "Keep Seeding"));

    let mut settings = test_settings();
    settings.seed_after_download = "always".to_string();
    settings.users[0].notify = false;

    let redis = RedisWrapper::new();
    redis.connect(None).await;

    crate::tasks::torrent_finished(None, None, &redis, &settings, &*client).await;

    let t = client.get_torrent(hash, None).await.unwrap().unwrap();
    assert_eq!(t.state, "completed");
}

#[tokio::test]
async fn test_finished_deduplicates_notifications_via_sqlite() {
    isolated_db();
    let client = Arc::new(MockTorrentClient::new());
    let hash = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    client.seed_torrent(completed_torrent(hash, "Already Notified"));

    let mut settings = test_settings();
    settings.seed_after_download = "never".to_string();
    settings.users[0].notify = false;

    let redis = RedisWrapper::new();
    redis.connect(None).await;

    crate::tasks::torrent_finished(None, None, &redis, &settings, &*client).await;

    client.seed_torrent(completed_torrent(hash, "Already Notified"));
    crate::tasks::torrent_finished(None, None, &redis, &settings, &*client).await;

    let t = client.get_torrent(hash, None).await.unwrap().unwrap();
    assert_eq!(t.state, "completed");
}

// -- Utility tests -------------------------------------------------------------

#[test]
fn test_extract_hash_from_magnet_standard() {
    let hash = "aabbccddeeff00112233445566778899aabbccdd";
    let magnet = format!("magnet:?xt=urn:btih:{}&dn=test&tr=udp://tracker.example.com", hash);
    assert_eq!(extract_hash_from_magnet(&magnet).as_deref(), Some(hash));
}

#[test]
fn test_extract_hash_no_btih_field() {
    assert_eq!(extract_hash_from_magnet("magnet:?dn=noxtfield"), None);
}

#[test]
fn test_extract_hash_stops_at_ampersand() {
    let h = extract_hash_from_magnet(
        "magnet:?xt=urn:btih:deadbeef1234567890abcdef1234567890abcdef&dn=name",
    )
    .unwrap();
    assert!(!h.contains('&'));
}
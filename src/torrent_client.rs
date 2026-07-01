use crate::aria2::Aria2Manager;
use crate::config::Settings;
use crate::qbittorrent::QBittorrentManager;
use crate::transmission::TransmissionManager;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Torrent {
    pub hash: String,
    pub name: String,
    pub progress: f64,
    pub dlspeed: u64,
    pub state: String,
    pub size: u64,
    pub eta: i64,
    pub category: Option<String>,
    pub save_path: Option<String>,
    pub content_path: Option<String>,
    pub num_seeds: Option<i64>,
    pub num_peers: Option<i64>,
}

#[async_trait]
pub trait TorrentClient: Send + Sync {
    async fn add_magnet(&self, magnet_link: &str, category: Option<&str>) -> Result<bool, String>;
    #[allow(dead_code)]
    async fn add_url(&self, _url: &str, _category: Option<&str>) -> Result<bool, String> {
        Err("Direct URL download is not supported by this client. Try downloading the torrent file first.".to_string())
    }
    async fn add_torrent(
        &self,
        file_bytes: Vec<u8>,
        filename: &str,
        category: Option<&str>,
    ) -> Result<bool, String>;
    async fn resume_all(&self) -> Result<(), String>;
    async fn pause_all(&self) -> Result<(), String>;
    async fn resume(&self, hash: &str) -> Result<(), String>;
    async fn pause(&self, hash: &str) -> Result<(), String>;
    async fn delete_one_no_data(&self, hash: &str) -> Result<(), String>;
    async fn delete_one_data(&self, hash: &str) -> Result<(), String>;
    async fn delete_all_no_data(&self) -> Result<(), String>;
    async fn delete_all_data(&self) -> Result<(), String>;
    async fn get_categories(&self) -> Result<Option<Vec<String>>, String>;
    async fn set_torrents_category(&self, category: &str, hashes: &str) -> Result<(), String>;
    async fn get_torrent(
        &self,
        hash: &str,
        status_filter: Option<&str>,
    ) -> Result<Option<Torrent>, String>;
    async fn get_torrents(
        &self,
        hash: Option<&str>,
        status_filter: Option<&str>,
    ) -> Result<Vec<Torrent>, String>;
    async fn edit_category(&self, name: &str, save_path: &str) -> Result<(), String>;
    async fn create_category(&self, name: &str, save_path: &str) -> Result<(), String>;
    async fn remove_category(&self, name: &str) -> Result<(), String>;
    async fn check_connection(&self) -> Result<String, String>;
    async fn export_torrent(&self, hash: &str) -> Result<(Vec<u8>, String), String>;
    async fn get_speed_limit_mode(&self) -> Result<bool, String>;
    async fn toggle_speed_limit(&self) -> Result<bool, String>;
}

pub fn create_client(settings: &Settings) -> Arc<dyn TorrentClient> {
    let host = settings.client.host.clone();
    let user = settings.client.user.clone();
    let pass = settings.client.password.clone();

    match settings.client.r#type.as_str() {
        "transmission" => Arc::new(TransmissionManager::new(&host, &user, &pass)),
        "aria2" => Arc::new(Aria2Manager::new(
            &host,
            &pass, // 'password' field used as RPC secret token
            settings.client.split.unwrap_or(5),
            settings.client.max_concurrent.unwrap_or(5),
        )),
        _ => Arc::new(QBittorrentManager::new(&host, &user, &pass)),
    }
}

use async_trait::async_trait;
use std::collections::BTreeMap as HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};

use crate::torrent_client::{Torrent, TorrentClient};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MockMethod {
    AddMagnet,
    AddTorrent,
    GetTorrent,
    GetTorrents,
    Pause,
    Resume,
    PauseAll,
    ResumeAll,
    DeleteOneNoData,
    DeleteOneData,
    CheckConnection,
    ToggleSpeedLimit,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct FileState {
    torrents: HashMap<String, Torrent>,
    next_errors: HashMap<MockMethod, String>,
    speed_limit_active: bool,
    next_id: u64,
}

pub struct MockTorrentClient {
    file_path: Option<String>,
    in_memory: Mutex<FileState>,
}

impl MockTorrentClient {
    pub fn new() -> Self {
        let path = std::env::var("MAGPIE_MOCK_FILE").ok();
        Self {
            file_path: path,
            in_memory: Mutex::new(FileState::default()),
        }
    }

    pub fn new_with_file(path: String) -> Self {
        Self {
            file_path: Some(path),
            in_memory: Mutex::new(FileState::default()),
        }
    }

    fn read_state(&self) -> FileState {
        if let Some(ref path) = self.file_path {
            if !Path::new(path).exists() {
                return FileState::default();
            }
            let content = fs::read_to_string(path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            self.in_memory.lock().unwrap().clone()
        }
    }

    fn write_state(&self, state: &FileState) {
        if let Some(ref path) = self.file_path {
            if let Some(parent) = Path::new(path).parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(content) = serde_json::to_string_pretty(state) {
                let _ = fs::write(path, content);
            }
        } else {
            *self.in_memory.lock().unwrap() = state.clone();
        }
    }

    pub fn seed_torrent(&self, t: Torrent) {
        let mut state = self.read_state();
        state.torrents.insert(t.hash.clone(), t);
        self.write_state(&state);
    }

    pub fn set_next_error(&self, method: MockMethod, message: impl Into<String>) {
        let mut state = self.read_state();
        state.next_errors.insert(method, message.into());
        self.write_state(&state);
    }

    pub fn all_torrents(&self) -> Vec<Torrent> {
        let state = self.read_state();
        state.torrents.values().cloned().collect()
    }

    fn take_error(state: &mut FileState, method: &MockMethod) -> Option<String> {
        state.next_errors.remove(method)
    }

    fn next_hash(state: &mut FileState) -> String {
        let id = state.next_id;
        state.next_id += 1;
        format!("{:016x}", id)
    }
}

#[async_trait]
impl TorrentClient for MockTorrentClient {
    async fn add_magnet(&self, magnet_link: &str, category: Option<&str>) -> Result<bool, String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::AddMagnet) {
            self.write_state(&state);
            return Err(e);
        }
        let hash = crate::utils::extract_hash_from_magnet(magnet_link)
            .unwrap_or_else(|| Self::next_hash(&mut state));
        if state.torrents.contains_key(&hash) {
            self.write_state(&state);
            return Err("409 Conflict: torrent already exists".to_string());
        }
        state.torrents.insert(
            hash.clone(),
            Torrent {
                hash,
                name: "Mock Torrent".to_string(),
                progress: 0.0,
                dlspeed: 1024 * 512,
                state: "downloading".to_string(),
                size: 1024 * 1024 * 100,
                eta: 200,
                category: category.map(|s| s.to_string()),
                save_path: Some("/downloads".to_string()),
                content_path: Some("/downloads/mock".to_string()),
                num_seeds: Some(5),
                num_peers: Some(10),
            },
        );
        self.write_state(&state);
        Ok(true)
    }

    async fn add_url(&self, url: &str, category: Option<&str>) -> Result<bool, String> {
        self.add_magnet(url, category).await
    }

    async fn add_torrent(
        &self,
        _file_bytes: Vec<u8>,
        filename: &str,
        category: Option<&str>,
    ) -> Result<bool, String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::AddTorrent) {
            self.write_state(&state);
            return Err(e);
        }
        let hash = Self::next_hash(&mut state);
        state.torrents.insert(
            hash.clone(),
            Torrent {
                hash,
                name: filename.trim_end_matches(".torrent").to_string(),
                progress: 0.0,
                dlspeed: 0,
                state: "downloading".to_string(),
                size: 1024 * 1024 * 200,
                eta: 3600,
                category: category.map(|s| s.to_string()),
                save_path: Some("/downloads".to_string()),
                content_path: None,
                num_seeds: None,
                num_peers: None,
            },
        );
        self.write_state(&state);
        Ok(true)
    }

    async fn resume_all(&self) -> Result<(), String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::ResumeAll) {
            self.write_state(&state);
            return Err(e);
        }
        for t in state.torrents.values_mut() {
            if t.state == "paused" {
                t.state = "downloading".to_string();
            }
        }
        self.write_state(&state);
        Ok(())
    }

    async fn pause_all(&self) -> Result<(), String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::PauseAll) {
            self.write_state(&state);
            return Err(e);
        }
        for t in state.torrents.values_mut() {
            t.state = "paused".to_string();
        }
        self.write_state(&state);
        Ok(())
    }

    async fn resume(&self, hash: &str) -> Result<(), String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::Resume) {
            self.write_state(&state);
            return Err(e);
        }
        match state.torrents.get_mut(hash) {
            Some(t) => {
                t.state = "downloading".to_string();
                self.write_state(&state);
                Ok(())
            }
            None => {
                self.write_state(&state);
                Err(format!("Torrent not found: {}", hash))
            }
        }
    }

    async fn pause(&self, hash: &str) -> Result<(), String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::Pause) {
            self.write_state(&state);
            return Err(e);
        }
        match state.torrents.get_mut(hash) {
            Some(t) => {
                t.state = "paused".to_string();
                self.write_state(&state);
                Ok(())
            }
            None => {
                self.write_state(&state);
                Err(format!("Torrent not found: {}", hash))
            }
        }
    }

    async fn delete_one_no_data(&self, hash: &str) -> Result<(), String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::DeleteOneNoData) {
            self.write_state(&state);
            return Err(e);
        }
        state.torrents.remove(hash);
        self.write_state(&state);
        Ok(())
    }

    async fn delete_one_data(&self, hash: &str) -> Result<(), String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::DeleteOneData) {
            self.write_state(&state);
            return Err(e);
        }
        state.torrents.remove(hash);
        self.write_state(&state);
        Ok(())
    }

    async fn delete_all_no_data(&self) -> Result<(), String> {
        let mut state = self.read_state();
        state.torrents.clear();
        self.write_state(&state);
        Ok(())
    }

    async fn delete_all_data(&self) -> Result<(), String> {
        let mut state = self.read_state();
        state.torrents.clear();
        self.write_state(&state);
        Ok(())
    }

    async fn get_categories(&self) -> Result<Option<Vec<String>>, String> {
        Ok(Some(vec!["movies".to_string(), "tv".to_string()]))
    }

    async fn set_torrents_category(&self, category: &str, hashes: &str) -> Result<(), String> {
        let mut state = self.read_state();
        for hash in hashes.split('|') {
            if let Some(t) = state.torrents.get_mut(hash.trim()) {
                t.category = Some(category.to_string());
            }
        }
        self.write_state(&state);
        Ok(())
    }

    async fn get_torrent(
        &self,
        hash: &str,
        _status_filter: Option<&str>,
    ) -> Result<Option<Torrent>, String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::GetTorrent) {
            self.write_state(&state);
            return Err(e);
        }
        let res = state.torrents.get(hash).cloned();
        self.write_state(&state);
        Ok(res)
    }

    async fn get_torrents(
        &self,
        hash: Option<&str>,
        status_filter: Option<&str>,
    ) -> Result<Vec<Torrent>, String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::GetTorrents) {
            self.write_state(&state);
            return Err(e);
        }
        let list = state
            .torrents
            .values()
            .filter(|t| {
                if let Some(h) = hash {
                    if t.hash != h { return false; }
                }
                if let Some(state) = status_filter {
                    return t.state == state;
                }
                true
            })
            .cloned()
            .collect();
        self.write_state(&state);
        Ok(list)
    }

    async fn edit_category(&self, _name: &str, _save_path: &str) -> Result<(), String> {
        Ok(())
    }

    async fn create_category(&self, _name: &str, _save_path: &str) -> Result<(), String> {
        Ok(())
    }

    async fn remove_category(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }

    async fn check_connection(&self) -> Result<String, String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::CheckConnection) {
            self.write_state(&state);
            return Err(e);
        }
        self.write_state(&state);
        Ok("MockTorrentClient v1.0".to_string())
    }

    async fn export_torrent(&self, _hash: &str) -> Result<(Vec<u8>, String), String> {
        Err("Export not supported by mock client".to_string())
    }

    async fn get_speed_limit_mode(&self) -> Result<bool, String> {
        let state = self.read_state();
        Ok(state.speed_limit_active)
    }

    async fn toggle_speed_limit(&self) -> Result<bool, String> {
        let mut state = self.read_state();
        if let Some(e) = Self::take_error(&mut state, &MockMethod::ToggleSpeedLimit) {
            self.write_state(&state);
            return Err(e);
        }
        state.speed_limit_active = !state.speed_limit_active;
        self.write_state(&state);
        Ok(state.speed_limit_active)
    }
}
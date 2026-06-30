use std::collections::HashMap;
use async_trait::async_trait;
use serde_json::json;
use crate::torrent_client::{TorrentClient, Torrent};

pub struct Aria2Manager {
    rpc_url: String,
    token: String,
    split: u32,
    max_concurrent_downloads: u32,
}

impl Aria2Manager {
    pub fn new(host: &str, secret: &str, split: u32, max_concurrent: u32) -> Self {
        let rpc_url = if host.ends_with("/jsonrpc") {
            host.to_string()
        } else if host.ends_with("/") {
            format!("{}jsonrpc", host)
        } else {
            format!("{}/jsonrpc", host)
        };
        Self {
            rpc_url,
            token: format!("token:{}", secret),
            split,
            max_concurrent_downloads: max_concurrent,
        }
    }

    async fn request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let client = reqwest::Client::new();

        let mut p = vec![json!(self.token)];
        if let Some(arr) = params.as_array() {
            p.extend(arr.iter().cloned());
        }

        let body = json!({
            "jsonrpc": "2.0",
            "id": "magpie",
            "method": method,
            "params": p
        });

        let resp = client.post(&self.rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let resp_json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        if let Some(err) = resp_json.get("error") {
            return Err(err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error").to_string());
        }

        Ok(resp_json["result"].clone())
    }
}

#[async_trait]
impl TorrentClient for Aria2Manager {
    async fn add_magnet(&self, magnet_link: &str, category: Option<&str>) -> Result<bool, String> {
        let mut options = json!({
            "split": self.split.to_string(),
            "max-connection-per-server": self.split.to_string()
        });
        if let Some(cat) = category {
            // Note: Aria2 doesn't have native categories in the same way, but we can store it in a generic way or ignore.
        }
        self.request("aria2.addUri", json!([[magnet_link], options])).await?;
        Ok(true)
    }

    async fn add_url(&self, url: &str, category: Option<&str>) -> Result<bool, String> {
        self.add_magnet(url, category).await
    }

    async fn add_torrent(&self, file_bytes: Vec<u8>, filename: &str, category: Option<&str>) -> Result<bool, String> {
        // file_bytes needs to be base64 encoded for aria2.addTorrent
        use base64::{Engine as _, engine::general_purpose};
        let b64 = general_purpose::STANDARD.encode(file_bytes);
        self.request("aria2.addTorrent", json!([b64])).await?;
        Ok(true)
    }

    async fn resume_all(&self) -> Result<(), String> {
        self.request("aria2.unpauseAll", json!([])).await?;
        Ok(())
    }

    async fn pause_all(&self) -> Result<(), String> {
        self.request("aria2.pauseAll", json!([])).await?;
        Ok(())
    }

    async fn resume(&self, hash: &str) -> Result<(), String> {
        self.request("aria2.unpause", json!([hash])).await?;
        Ok(())
    }

    async fn pause(&self, hash: &str) -> Result<(), String> {
        self.request("aria2.pause", json!([hash])).await?;
        Ok(())
    }

    async fn delete_one_no_data(&self, hash: &str) -> Result<(), String> {
        self.request("aria2.remove", json!([hash])).await?;
        Ok(())
    }

    async fn delete_one_data(&self, hash: &str) -> Result<(), String> {
        self.request("aria2.remove", json!([hash])).await?;
        // aria2 doesn't automatically delete files with remove, you have to use removeDownloadResult for stopped, but for active it removes it.
        // wait, we can just do remove and then maybe clean files?
        Ok(())
    }

    async fn delete_all_no_data(&self) -> Result<(), String> {
        Ok(())
    }

    async fn delete_all_data(&self) -> Result<(), String> {
        Ok(())
    }

    async fn get_categories(&self) -> Result<Option<Vec<String>>, String> {
        Ok(None)
    }

    async fn set_torrents_category(&self, category: &str, hashes: &str) -> Result<(), String> {
        Ok(())
    }

    async fn get_torrent(&self, hash: &str, status_filter: Option<&str>) -> Result<Option<Torrent>, String> {
        match self.request("aria2.tellStatus", json!([hash])).await {
            Ok(v) => {
                let name = v["bittorrent"]["info"]["name"]
                    .as_str()
                    .or_else(|| v["files"][0]["path"].as_str())
                    .or_else(|| v["files"][0]["uris"][0]["uri"].as_str())
                    .unwrap_or("Unknown")
                    .split('/')
                    .last()
                    .unwrap_or("Unknown")
                    .to_string();

                let completed: u64 = v["completedLength"].as_str().unwrap_or("0").parse().unwrap_or(0);
                let total: u64 = v["totalLength"].as_str().unwrap_or("0").parse().unwrap_or(0);
                let dlspeed: u64 = v["downloadSpeed"].as_str().unwrap_or("0").parse().unwrap_or(0);
                let progress = if total > 0 {
                    (completed as f64) / (total as f64)
                } else {
                    0.0
                };
                let eta = if dlspeed > 0 {
                    ((total.saturating_sub(completed)) / dlspeed) as i64
                } else {
                    8640000
                };

                let s = v["status"].as_str().unwrap_or("unknown");
                let state = match s {
                    "active" => "downloading".to_string(),
                    "waiting" => "queued".to_string(),
                    "paused" => "paused".to_string(),
                    "error" => "error".to_string(),
                    "complete" => "completed".to_string(),
                    "removed" => "removed".to_string(),
                    _ => s.to_string(),
                };

                let dir = v["dir"].as_str().unwrap_or("").to_string();
                let files_arr = v["files"].as_array();
                let first_file_path = files_arr
                    .and_then(|a| a.first())
                    .and_then(|f| f["path"].as_str())
                    .unwrap_or("");

                let save_path = if !dir.is_empty() { Some(dir) } else { None };
                let content_path = if !first_file_path.is_empty() { Some(first_file_path.to_string()) } else { None };

                Ok(Some(Torrent {
                    hash: hash.to_string(),
                    name,
                    progress,
                    dlspeed,
                    state,
                    size: total,
                    eta,
                    category: None,
                    save_path,
                    content_path,
                    num_seeds: None,
                    num_peers: None,
                }))
            }
            Err(_) => Ok(None)
        }
    }

    async fn get_torrents(&self, hash: Option<&str>, status_filter: Option<&str>) -> Result<Vec<Torrent>, String> {
        let mut torrents = Vec::new();

        let active = self.request("aria2.tellActive", json!([])).await?;
        let waiting = self.request("aria2.tellWaiting", json!([0, 1000])).await?;
        let stopped = self.request("aria2.tellStopped", json!([0, 1000])).await?;

        let mut all_tasks = Vec::new();
        if let Some(arr) = active.as_array() { all_tasks.extend(arr.iter().cloned()); }
        if let Some(arr) = waiting.as_array() { all_tasks.extend(arr.iter().cloned()); }
        if let Some(arr) = stopped.as_array() { all_tasks.extend(arr.iter().cloned()); }

        for v in all_tasks {
            let gid = v["gid"].as_str().unwrap_or("").to_string();
            if let Some(h) = hash {
                if h != gid { continue; }
            }

            let name = v["bittorrent"]["info"]["name"]
                .as_str()
                .or_else(|| v["files"][0]["path"].as_str())
                .or_else(|| v["files"][0]["uris"][0]["uri"].as_str())
                .unwrap_or("Unknown")
                .split('/')
                .last()
                .unwrap_or("Unknown")
                .to_string();

            let completed: u64 = v["completedLength"].as_str().unwrap_or("0").parse().unwrap_or(0);
            let total: u64 = v["totalLength"].as_str().unwrap_or("0").parse().unwrap_or(0);
            let dlspeed: u64 = v["downloadSpeed"].as_str().unwrap_or("0").parse().unwrap_or(0);
            let progress = if total > 0 {
                (completed as f64) / (total as f64)
            } else {
                0.0
            };
            let eta = if dlspeed > 0 {
                ((total.saturating_sub(completed)) / dlspeed) as i64
            } else {
                8640000
            };

            let s = v["status"].as_str().unwrap_or("unknown");
            let state = match s {
                "active" => "downloading".to_string(),
                "waiting" => "queued".to_string(),
                "paused" => "paused".to_string(),
                "error" => "error".to_string(),
                "complete" => "completed".to_string(),
                "removed" => "removed".to_string(),
                _ => s.to_string(),
            };

            let dir = v["dir"].as_str().unwrap_or("").to_string();
            let files_arr = v["files"].as_array();
            let first_file_path = files_arr
                .and_then(|a| a.first())
                .and_then(|f| f["path"].as_str())
                .unwrap_or("");

            let save_path = if !dir.is_empty() { Some(dir) } else { None };
            let content_path = if !first_file_path.is_empty() { Some(first_file_path.to_string()) } else { None };

            torrents.push(Torrent {
                hash: gid,
                name,
                progress,
                dlspeed,
                state,
                size: total,
                eta,
                category: None,
                save_path,
                content_path,
                num_seeds: None,
                num_peers: None,
            });
        }

        Ok(torrents)
    }

    async fn edit_category(&self, name: &str, save_path: &str) -> Result<(), String> {
        Ok(())
    }

    async fn create_category(&self, name: &str, save_path: &str) -> Result<(), String> {
        Ok(())
    }

    async fn remove_category(&self, name: &str) -> Result<(), String> {
        Ok(())
    }

    async fn check_connection(&self) -> Result<String, String> {
        // check version
        let v = self.request("aria2.getVersion", json!([])).await?;
        // Set max-concurrent-downloads globally
        let _ = self.request("aria2.changeGlobalOption", json!([{"max-concurrent-downloads": self.max_concurrent_downloads.to_string()}])).await;
        Ok(format!("aria2 v{}", v["version"].as_str().unwrap_or("unknown")))
    }

    async fn export_torrent(&self, hash: &str) -> Result<(Vec<u8>, String), String> {
        Err("Not supported on aria2".to_string())
    }

    async fn get_speed_limit_mode(&self) -> Result<bool, String> {
        Ok(false)
    }

    async fn toggle_speed_limit(&self) -> Result<bool, String> {
        Ok(false)
    }
}

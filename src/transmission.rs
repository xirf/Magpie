use crate::torrent_client::{Torrent, TorrentClient};
use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct TransmissionManager {
    host: String,
    user: String,
    pass: String,
    client: Client,
    session_id: Arc<RwLock<Option<String>>>,
}

impl TransmissionManager {
    pub fn new(host: &str, user: &str, pass: &str) -> Self {
        let clean_host = host.trim_end_matches('/').to_string();
        Self {
            host: clean_host,
            user: user.to_string(),
            pass: pass.to_string(),
            client: Client::builder().build().unwrap(),
            session_id: Arc::new(RwLock::new(None)),
        }
    }

    async fn request(
        &self,
        method: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let body = serde_json::json!({
            "method": method,
            "arguments": arguments,
        });

        for _ in 0..2 {
            let mut req = self
                .client
                .post(format!("{}/transmission/rpc", self.host))
                .json(&body);

            if !self.user.is_empty() {
                req = req.basic_auth(&self.user, Some(&self.pass));
            }

            {
                let sid_read = self.session_id.read().await;
                if let Some(ref sid) = *sid_read {
                    req = req.header("X-Transmission-Session-Id", sid);
                }
            }

            let res = req.send().await.map_err(|e| e.to_string())?;
            if res.status().as_u16() == 409 {
                if let Some(new_sid) = res.headers().get("X-Transmission-Session-Id") {
                    if let Ok(new_sid_str) = new_sid.to_str() {
                        *self.session_id.write().await = Some(new_sid_str.to_string());
                        continue;
                    }
                }
                return Err(
                    "Failed to get X-Transmission-Session-Id header from 409 response".to_string(),
                );
            }

            if !res.status().is_success() {
                let status = res.status();
                let txt = res.text().await.unwrap_or_default();
                return Err(format!("Transmission RPC error ({}): {}", status, txt));
            }

            let resp_json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
            if let Some(result) = resp_json.get("result") {
                if result.as_str() != Some("success") {
                    return Err(format!("Transmission RPC failure: {:?}", result));
                }
            }
            if let Some(arguments) = resp_json.get("arguments") {
                return Ok(arguments.clone());
            }
            return Ok(resp_json);
        }

        Err("Failed to execute RPC request after session ID refresh".to_string())
    }
}

#[derive(Deserialize, Debug)]
struct TransmissionTorrent {
    #[serde(rename = "id")]
    #[allow(dead_code)]
    id: i64,
    #[serde(rename = "hashString")]
    hash_string: String,
    name: String,
    #[serde(rename = "percentDone")]
    percent_done: f64,
    #[serde(rename = "rateDownload")]
    rate_download: u64,
    status: i64,
    #[serde(rename = "totalSize")]
    total_size: u64,
    eta: i64,
    #[serde(rename = "downloadDir")]
    download_dir: String,
    labels: Option<Vec<String>>,
    #[serde(rename = "peersConnected")]
    peers_connected: i64,
    #[serde(rename = "peersSendingToUs")]
    peers_sending_to_us: i64,
}

#[async_trait]
impl TorrentClient for TransmissionManager {
    async fn add_magnet(&self, magnet_link: &str, category: Option<&str>) -> Result<bool, String> {
        let mut args = serde_json::json!({
            "filename": magnet_link,
            "paused": false,
        });

        if let Some(cat) = category {
            args["labels"] = serde_json::json!([cat]);
        }

        let resp = self.request("torrent-add", args).await?;
        Ok(resp.get("torrent-added").is_some()
            || resp.get("torrent-duplicate").is_some()
            || resp.get("torrent-added-row").is_some())
    }

    async fn add_torrent(
        &self,
        file_bytes: Vec<u8>,
        _filename: &str,
        category: Option<&str>,
    ) -> Result<bool, String> {
        let base64_data = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
        let mut args = serde_json::json!({
            "metainfo": base64_data,
            "paused": false,
        });

        if let Some(cat) = category {
            args["labels"] = serde_json::json!([cat]);
        }

        let resp = self.request("torrent-add", args).await?;
        Ok(resp.get("torrent-added").is_some()
            || resp.get("torrent-duplicate").is_some()
            || resp.get("torrent-added-row").is_some())
    }

    async fn resume_all(&self) -> Result<(), String> {
        self.request("torrent-start", serde_json::json!({})).await?;
        Ok(())
    }

    async fn pause_all(&self) -> Result<(), String> {
        self.request("torrent-stop", serde_json::json!({})).await?;
        Ok(())
    }

    async fn resume(&self, hash: &str) -> Result<(), String> {
        self.request("torrent-start", serde_json::json!({ "ids": [hash] }))
            .await?;
        Ok(())
    }

    async fn pause(&self, hash: &str) -> Result<(), String> {
        self.request("torrent-stop", serde_json::json!({ "ids": [hash] }))
            .await?;
        Ok(())
    }

    async fn delete_one_no_data(&self, hash: &str) -> Result<(), String> {
        self.request(
            "torrent-remove",
            serde_json::json!({
                "ids": [hash],
                "delete-local-data": false
            }),
        )
        .await?;
        Ok(())
    }

    async fn delete_one_data(&self, hash: &str) -> Result<(), String> {
        self.request(
            "torrent-remove",
            serde_json::json!({
                "ids": [hash],
                "delete-local-data": true
            }),
        )
        .await?;
        Ok(())
    }

    async fn delete_all_no_data(&self) -> Result<(), String> {
        let torrents = self.get_torrents(None, None).await?;
        let hashes: Vec<String> = torrents.into_iter().map(|t| t.hash).collect();
        if !hashes.is_empty() {
            self.request(
                "torrent-remove",
                serde_json::json!({
                    "ids": hashes,
                    "delete-local-data": false
                }),
            )
            .await?;
        }
        Ok(())
    }

    async fn delete_all_data(&self) -> Result<(), String> {
        let torrents = self.get_torrents(None, None).await?;
        let hashes: Vec<String> = torrents.into_iter().map(|t| t.hash).collect();
        if !hashes.is_empty() {
            self.request(
                "torrent-remove",
                serde_json::json!({
                    "ids": hashes,
                    "delete-local-data": true
                }),
            )
            .await?;
        }
        Ok(())
    }

    async fn get_categories(&self) -> Result<Option<Vec<String>>, String> {
        let torrents = self.get_torrents(None, None).await?;
        let mut cats = std::collections::BTreeSet::new();
        for t in torrents {
            if let Some(cat) = t.category {
                cats.insert(cat);
            }
        }
        let list: Vec<String> = cats.into_iter().collect();
        if list.is_empty() {
            Ok(None)
        } else {
            Ok(Some(list))
        }
    }

    async fn set_torrents_category(&self, category: &str, hashes: &str) -> Result<(), String> {
        let hash_list: Vec<&str> = hashes.split('|').collect();
        self.request(
            "torrent-set",
            serde_json::json!({
                "ids": hash_list,
                "labels": [category]
            }),
        )
        .await?;
        Ok(())
    }

    async fn get_torrent(
        &self,
        hash: &str,
        status_filter: Option<&str>,
    ) -> Result<Option<Torrent>, String> {
        let torrents = self.get_torrents(Some(hash), status_filter).await?;
        if torrents.is_empty() {
            Ok(None)
        } else {
            Ok(Some(torrents[0].clone()))
        }
    }

    async fn get_torrents(
        &self,
        hash: Option<&str>,
        status_filter: Option<&str>,
    ) -> Result<Vec<Torrent>, String> {
        let fields = vec![
            "id",
            "hashString",
            "name",
            "percentDone",
            "rateDownload",
            "status",
            "totalSize",
            "eta",
            "downloadDir",
            "labels",
            "peersConnected",
            "peersSendingToUs",
        ];
        let mut args = serde_json::json!({
            "fields": fields
        });

        if let Some(h) = hash {
            args["ids"] = serde_json::json!([h]);
        }

        let resp = self.request("torrent-get", args).await?;
        let raw_torrents = resp.get("torrents").ok_or("No torrents in response")?;
        let list: Vec<TransmissionTorrent> =
            serde_json::from_value(raw_torrents.clone()).map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        for t in list {
            let state = match t.status {
                0 => "paused",
                1 | 2 => "checking",
                3 | 4 => "downloading",
                5 | 6 => "seeding",
                _ => "unknown",
            }
            .to_string();

            let category = t.labels.and_then(|l| l.first().cloned());
            let save_path = Some(t.download_dir.clone());
            let content_path = Some(format!("{}/{}", t.download_dir, t.name));
            let progress = t.percent_done;

            if let Some(filter) = status_filter {
                match filter {
                    "completed" => {
                        if progress < 1.0 {
                            continue;
                        }
                    }
                    "downloading" => {
                        if progress >= 1.0 || (t.status != 3 && t.status != 4) {
                            continue;
                        }
                    }
                    "paused" | "stopped" => {
                        if t.status != 0 {
                            continue;
                        }
                    }
                    "seeding" | "active" => {
                        if t.status != 5 && t.status != 6 {
                            continue;
                        }
                    }
                    _ => {}
                }
            }

            results.push(Torrent {
                hash: t.hash_string,
                name: t.name,
                progress,
                dlspeed: t.rate_download,
                state,
                size: t.total_size,
                eta: t.eta,
                category,
                save_path,
                content_path,
                num_seeds: Some(t.peers_sending_to_us),
                num_peers: Some(t.peers_connected),
            });
        }

        Ok(results)
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
        let resp = self.request("session-get", serde_json::json!({})).await?;
        if let Some(version) = resp.get("version") {
            Ok(version.as_str().unwrap_or("Transmission").to_string())
        } else {
            Ok("Transmission".to_string())
        }
    }

    async fn export_torrent(&self, _hash: &str) -> Result<(Vec<u8>, String), String> {
        Err("Export torrent is not supported by Transmission RPC API".to_string())
    }

    async fn get_speed_limit_mode(&self) -> Result<bool, String> {
        let resp = self.request("session-get", serde_json::json!({})).await?;
        let active = resp
            .get("alt-speed-enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Ok(active)
    }

    async fn toggle_speed_limit(&self) -> Result<bool, String> {
        let current = self.get_speed_limit_mode().await?;
        let next = !current;
        self.request(
            "session-set",
            serde_json::json!({
                "alt-speed-enabled": next
            }),
        )
        .await?;
        Ok(next)
    }
}

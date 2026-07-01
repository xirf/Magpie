use async_trait::async_trait;
use serde_json::json;
use crate::torrent_client::{TorrentClient, Torrent};

/// Generates a short unique hex string for temp file names (no extra deps needed).
fn uuid_hex() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    // Mix in a thread-local pseudo-random value via hash of address
    let addr = &t as *const _ as usize;
    format!("{:x}{:x}", t, addr)
}

pub struct Aria2Manager {
    rpc_url: String,
    token: String,
    split: u32,
    max_concurrent_downloads: u32,
    client: reqwest::Client,
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
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
        }
    }

    async fn request(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
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

        let resp = self.client.post(&self.rpc_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                eprintln!("[Aria2 RPC Error] Failed to send request to {}. Error: {:?}. Is connect error: {}, Is timeout: {}, Source: {:?}", self.rpc_url, e, e.is_connect(), e.is_timeout(), std::error::Error::source(&e));
                e.to_string()
            })?;

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
        println!("[Aria2Manager] add_magnet: magnet_link={}, category={:?}", magnet_link, category);
        let options = json!({
            "split": self.split.to_string(),
            "max-connection-per-server": self.split.to_string()
        });
        if let Some(_cat) = category {
            // Note: Aria2 doesn't have native categories in the same way, but we can store it in a generic way or ignore.
        }
        let res = self.request("aria2.addUri", json!([[magnet_link], options])).await;
        let final_res = match res {
            Ok(_) => Ok(true),
            Err(e) => Err(e),
        };
        println!("[Aria2Manager] add_magnet result: {:?}", final_res);
        final_res
    }

    async fn add_url(&self, url: &str, category: Option<&str>) -> Result<bool, String> {
        println!("[Aria2Manager] add_url: url={}, category={:?}", url, category);
        let res = self.add_magnet(url, category).await;
        println!("[Aria2Manager] add_url result: {:?}", res);
        res
    }

    async fn add_torrent(&self, file_bytes: Vec<u8>, filename: &str, category: Option<&str>) -> Result<bool, String> {
        println!("[Aria2Manager] add_torrent: filename={}, bytes_len={}, category={:?}", filename, file_bytes.len(), category);

        // aria2's RPC has a default max-request-size of 2MB. Large .torrent files encoded
        // as base64 can exceed this, causing a "Broken pipe" error. To work around this,
        // we write the torrent to a temp file and pass a file:// URI to aria2.addUri instead.
        let tmp_path = std::env::temp_dir().join(format!("magpie_{}.torrent", uuid_hex()));
        if let Err(e) = std::fs::write(&tmp_path, &file_bytes) {
            return Err(format!("Failed to write temp torrent file: {}", e));
        }

        let uri = format!("file://{}", tmp_path.to_string_lossy());
        let options = serde_json::json!({
            "split": self.split.to_string(),
            "max-connection-per-server": self.split.to_string()
        });
        let res = self.request("aria2.addUri", serde_json::json!([[uri], options])).await;

        // Clean up temp file regardless of outcome
        let _ = std::fs::remove_file(&tmp_path);

        let final_res = match res {
            Ok(_) => Ok(true),
            Err(e) => Err(e),
        };
        println!("[Aria2Manager] add_torrent result: {:?}", final_res);
        final_res
    }

    async fn resume_all(&self) -> Result<(), String> {
        println!("[Aria2Manager] resume_all");
        let res = self.request("aria2.unpauseAll", json!([])).await;
        let final_res = match res {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        println!("[Aria2Manager] resume_all result: {:?}", final_res);
        final_res
    }

    async fn pause_all(&self) -> Result<(), String> {
        println!("[Aria2Manager] pause_all");
        let res = self.request("aria2.pauseAll", json!([])).await;
        let final_res = match res {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        println!("[Aria2Manager] pause_all result: {:?}", final_res);
        final_res
    }

    async fn resume(&self, hash: &str) -> Result<(), String> {
        println!("[Aria2Manager] resume: hash={}", hash);
        let res = self.request("aria2.unpause", json!([hash])).await;
        let final_res = match res {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.contains("cannot be unpaused now") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        };
        println!("[Aria2Manager] resume result: {:?}", final_res);
        final_res
    }

    async fn pause(&self, hash: &str) -> Result<(), String> {
        println!("[Aria2Manager] pause: hash={}", hash);
        let res = self.request("aria2.pause", json!([hash])).await;
        let final_res = match res {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.contains("cannot be paused now") {
                    Ok(())
                } else {
                    Err(e)
                }
            }
        };
        println!("[Aria2Manager] pause result: {:?}", final_res);
        final_res
    }

    async fn delete_one_no_data(&self, hash: &str) -> Result<(), String> {
        println!("[Aria2Manager] delete_one_no_data: hash={}", hash);
        let res = self.request("aria2.remove", json!([hash])).await;
        let final_res = match res {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        println!("[Aria2Manager] delete_one_no_data result: {:?}", final_res);
        final_res
    }

    async fn delete_one_data(&self, hash: &str) -> Result<(), String> {
        println!("[Aria2Manager] delete_one_data: hash={}", hash);
        let res = self.request("aria2.remove", json!([hash])).await;
        let final_res = match res {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        println!("[Aria2Manager] delete_one_data result: {:?}", final_res);
        final_res
    }

    async fn delete_all_no_data(&self) -> Result<(), String> {
        println!("[Aria2Manager] delete_all_no_data");
        Ok(())
    }

    async fn delete_all_data(&self) -> Result<(), String> {
        println!("[Aria2Manager] delete_all_data");
        Ok(())
    }

    async fn get_categories(&self) -> Result<Option<Vec<String>>, String> {
        Ok(None)
    }

    async fn set_torrents_category(&self, category: &str, hashes: &str) -> Result<(), String> {
        println!("[Aria2Manager] set_torrents_category: category={}, hashes={}", category, hashes);
        Ok(())
    }

    async fn get_torrent(&self, hash: &str, status_filter: Option<&str>) -> Result<Option<Torrent>, String> {
        let res = match self.request("aria2.tellStatus", json!([hash])).await {
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
                if s == "error" {
                    let err_code = v["errorCode"].as_str().unwrap_or("unknown");
                    let err_msg = v["errorMessage"].as_str().unwrap_or("no message");
                    println!("[Aria2Manager] Task {} failed with errorCode: {}, errorMessage: {}", hash, err_code, err_msg);
                }
                let state = match s {
                    "active" => "downloading".to_string(),
                    "waiting" => "queued".to_string(),
                    "paused" => "paused".to_string(),
                    "error" => "error".to_string(),
                    "complete" => "completed".to_string(),
                    "removed" => "removed".to_string(),
                    _ => s.to_string(),
                };

                if let Some(filter) = status_filter {
                    if state != filter {
                        return Ok(None);
                    }
                }

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
            Err(e) => Err(e)
        };
        if let Err(ref e) = res {
            println!("[Aria2Manager] get_torrent hash={} error: {}", hash, e);
        }
        res
    }

    async fn get_torrents(&self, hash: Option<&str>, status_filter: Option<&str>) -> Result<Vec<Torrent>, String> {
        let active = self.request("aria2.tellActive", json!([])).await;
        let waiting = self.request("aria2.tellWaiting", json!([0, 1000])).await;
        let stopped = self.request("aria2.tellStopped", json!([0, 1000])).await;

        let active_val = match active {
            Ok(v) => v,
            Err(e) => {
                println!("[Aria2Manager] get_torrents (tellActive) error: {}", e);
                return Err(e);
            }
        };
        let waiting_val = match waiting {
            Ok(v) => v,
            Err(e) => {
                println!("[Aria2Manager] get_torrents (tellWaiting) error: {}", e);
                return Err(e);
            }
        };
        let stopped_val = match stopped {
            Ok(v) => v,
            Err(e) => {
                println!("[Aria2Manager] get_torrents (tellStopped) error: {}", e);
                return Err(e);
            }
        };

        let mut all_tasks = Vec::new();
        if let Some(arr) = active_val.as_array() { all_tasks.extend(arr.iter().cloned()); }
        if let Some(arr) = waiting_val.as_array() { all_tasks.extend(arr.iter().cloned()); }
        if let Some(arr) = stopped_val.as_array() { all_tasks.extend(arr.iter().cloned()); }

        let mut torrents = Vec::new();
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
            if s == "error" {
                let err_code = v["errorCode"].as_str().unwrap_or("unknown");
                let err_msg = v["errorMessage"].as_str().unwrap_or("no message");
                println!("[Aria2Manager] Task {} failed with errorCode: {}, errorMessage: {}", gid, err_code, err_msg);
            }
            let state = match s {
                "active" => "downloading".to_string(),
                "waiting" => "queued".to_string(),
                "paused" => "paused".to_string(),
                "error" => "error".to_string(),
                "complete" => "completed".to_string(),
                "removed" => "removed".to_string(),
                _ => s.to_string(),
            };

            if let Some(filter) = status_filter {
                if state != filter {
                    continue;
                }
            }

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
        println!("[Aria2Manager] edit_category: name={}, save_path={}", name, save_path);
        Ok(())
    }

    async fn create_category(&self, name: &str, save_path: &str) -> Result<(), String> {
        println!("[Aria2Manager] create_category: name={}, save_path={}", name, save_path);
        Ok(())
    }

    async fn remove_category(&self, name: &str) -> Result<(), String> {
        println!("[Aria2Manager] remove_category: name={}", name);
        Ok(())
    }

    async fn check_connection(&self) -> Result<String, String> {
        println!("[Aria2Manager] check_connection");
        // check version
        let v = self.request("aria2.getVersion", json!([])).await?;
        // Set max-concurrent-downloads globally
        let _ = self.request("aria2.changeGlobalOption", json!([{"max-concurrent-downloads": self.max_concurrent_downloads.to_string()}])).await;
        let version_str = format!("aria2 v{}", v["version"].as_str().unwrap_or("unknown"));
        println!("[Aria2Manager] check_connection result: {:?}", version_str);
        Ok(version_str)
    }

    async fn export_torrent(&self, hash: &str) -> Result<(Vec<u8>, String), String> {
        println!("[Aria2Manager] export_torrent: hash={}", hash);
        Err("Not supported on aria2".to_string())
    }

    async fn get_speed_limit_mode(&self) -> Result<bool, String> {
        Ok(false)
    }

    async fn toggle_speed_limit(&self) -> Result<bool, String> {
        println!("[Aria2Manager] toggle_speed_limit");
        Ok(false)
    }
}

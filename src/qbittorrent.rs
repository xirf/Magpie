use crate::torrent_client::{Torrent, TorrentClient};
use async_trait::async_trait;
use reqwest::header::COOKIE;
use reqwest::{Client, Response};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct QBittorrentManager {
    host: String,
    user: String,
    pass: String,
    client: Client,
    sid: Arc<RwLock<Option<String>>>,
    is_v5: Arc<RwLock<Option<bool>>>,
}

impl QBittorrentManager {
    pub fn new(host: &str, user: &str, pass: &str) -> Self {
        let clean_host = host.trim_end_matches('/').to_string();
        Self {
            host: clean_host,
            user: user.to_string(),
            pass: pass.to_string(),
            client: Client::builder().build().unwrap(),
            sid: Arc::new(RwLock::new(None)),
            is_v5: Arc::new(RwLock::new(None)),
        }
    }

    async fn get_is_v5(&self) -> bool {
        {
            let is_v5_opt = *self.is_v5.read().await;
            if let Some(val) = is_v5_opt {
                return val;
            }
        }

        let val = match self.check_connection().await {
            Ok(version) => {
                let ver = version.trim().to_lowercase();
                ver.starts_with('v') && ver.chars().nth(1).is_some_and(|c| c >= '5')
            }
            Err(_) => false,
        };

        *self.is_v5.write().await = Some(val);
        val
    }

    async fn login(&self) -> Result<(), String> {
        let mut params = HashMap::new();
        params.insert("username", &self.user);
        params.insert("password", &self.pass);

        let url = format!("{}/api/v2/auth/login", self.host);
        let res = self
            .client
            .post(&url)
            .form(&params)
            .header("Referer", &self.host)
            .header("Origin", &self.host)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !res.status().is_success() {
            let txt = res.text().await.unwrap_or_default();
            return Err(format!("Login failed: {}", txt));
        }

        let mut sid_val = None;
        if let Some(cookie_hdr) = res.headers().get("set-cookie") {
            if let Ok(cookie_str) = cookie_hdr.to_str() {
                for part in cookie_str.split(';') {
                    let trim_part = part.trim();
                    if trim_part.to_lowercase().starts_with("qbt_sid_")
                        || trim_part.to_lowercase().starts_with("sid=")
                    {
                        sid_val = Some(trim_part.to_string());
                        break;
                    }
                }
            }
        }

        let sid = sid_val.ok_or_else(|| "No SID cookie returned".to_string())?;
        *self.sid.write().await = Some(sid);
        Ok(())
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Vec<u8>>,
        form: Option<HashMap<&str, String>>,
        mut multipart: Option<reqwest::multipart::Form>,
    ) -> Result<Response, String> {
        let mut retries = 0;
        loop {
            let sid_opt = self.sid.read().await.clone();
            let sid = match sid_opt {
                Some(s) => s,
                None => {
                    self.login().await?;
                    self.sid
                        .read()
                        .await
                        .clone()
                        .ok_or_else(|| "Failed to login".to_string())?
                }
            };

            let url = format!("{}{}", self.host, path);
            let mut req = self
                .client
                .request(method.clone(), &url)
                .header(COOKIE, &sid)
                .header("Referer", &self.host)
                .header("Origin", &self.host);

            if let Some(ref b) = body {
                req = req.body(b.clone());
            } else if let Some(ref f) = form {
                req = req.form(f);
            } else if let Some(m) = multipart.take() {
                req = req.multipart(m);
            }

            let res = req.send().await.map_err(|e| e.to_string())?;

            if res.status() == reqwest::StatusCode::FORBIDDEN && retries < 1 {
                *self.sid.write().await = None;
                retries += 1;
                continue;
            }

            if !res.status().is_success() {
                return Err(format!(
                    "Request failed with status {}: {}",
                    res.status(),
                    res.text().await.unwrap_or_default()
                ));
            }

            return Ok(res);
        }
    }
}

#[async_trait]
impl TorrentClient for QBittorrentManager {
    async fn add_magnet(&self, magnet_link: &str, category: Option<&str>) -> Result<bool, String> {
        let mut form = HashMap::new();
        form.insert("urls", magnet_link.to_string());
        if let Some(cat) = category {
            if cat != "None" {
                form.insert("category", cat.to_string());
            }
        }

        let _ = self
            .request(
                reqwest::Method::POST,
                "/api/v2/torrents/add",
                None,
                Some(form),
                None,
            )
            .await?;
        Ok(true)
    }

    async fn add_torrent(
        &self,
        file_bytes: Vec<u8>,
        filename: &str,
        category: Option<&str>,
    ) -> Result<bool, String> {
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(filename.to_string())
            .mime_str("application/x-bittorrent")
            .map_err(|e| e.to_string())?;

        let mut form = reqwest::multipart::Form::new().part("torrents", part);
        if let Some(cat) = category {
            if cat != "None" {
                form = form.text("category", cat.to_string());
            }
        }

        let _ = self
            .request(
                reqwest::Method::POST,
                "/api/v2/torrents/add",
                None,
                None,
                Some(form),
            )
            .await?;
        Ok(true)
    }

    async fn resume_all(&self) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", "all".to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/resume",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn pause_all(&self) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", "all".to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/pause",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn resume(&self, hash: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", hash.to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/resume",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn pause(&self, hash: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", hash.to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/pause",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn delete_one_no_data(&self, hash: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", hash.to_string());
        form.insert("deleteFiles", "false".to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/delete",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn delete_one_data(&self, hash: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", hash.to_string());
        form.insert("deleteFiles", "true".to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/delete",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn delete_all_no_data(&self) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", "all".to_string());
        form.insert("deleteFiles", "false".to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/delete",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn delete_all_data(&self) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", "all".to_string());
        form.insert("deleteFiles", "true".to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/delete",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn get_categories(&self) -> Result<Option<Vec<String>>, String> {
        let res = self
            .request(
                reqwest::Method::GET,
                "/api/v2/torrents/categories",
                None,
                None,
                None,
            )
            .await?;

        #[derive(Deserialize)]
        struct CategoryDetail {
            _name: String,
        }

        let cats: HashMap<String, CategoryDetail> = res.json().await.map_err(|e| e.to_string())?;
        if cats.is_empty() {
            Ok(None)
        } else {
            let list = cats.keys().cloned().collect();
            Ok(Some(list))
        }
    }

    async fn set_torrents_category(&self, category: &str, hashes: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("hashes", hashes.to_string());
        form.insert("category", category.to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/setCategory",
            None,
            Some(form),
            None,
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
        let mut query_params = Vec::new();
        if let Some(h) = hash {
            query_params.push(format!("hashes={}", h));
        }
        if let Some(mut filter) = status_filter {
            let is_v5 = self.get_is_v5().await;
            if is_v5 {
                if filter == "paused" {
                    filter = "stopped";
                } else if filter == "resumed" {
                    filter = "running";
                }
            }
            query_params.push(format!("filter={}", filter));
        }

        let query = if query_params.is_empty() {
            "".to_string()
        } else {
            format!("?{}", query_params.join("&"))
        };

        let path = format!("/api/v2/torrents/info{}", query);
        let res = self
            .request(reqwest::Method::GET, &path, None, None, None)
            .await?;

        #[derive(Deserialize)]
        struct QBTorrent {
            hash: String,
            name: String,
            progress: f64,
            dlspeed: u64,
            state: String,
            size: u64,
            eta: i64,
            category: Option<String>,
            save_path: Option<String>,
            content_path: Option<String>,
            num_seeds: i64,
            num_leechers: i64,
        }

        let data: Vec<QBTorrent> = res.json().await.map_err(|e| e.to_string())?;

        Ok(data
            .into_iter()
            .map(|t| Torrent {
                hash: t.hash,
                name: t.name,
                progress: t.progress,
                dlspeed: t.dlspeed,
                state: t.state,
                size: t.size,
                eta: t.eta,
                category: if t.category.as_deref() == Some("") {
                    None
                } else {
                    t.category
                },
                save_path: t.save_path,
                content_path: t.content_path,
                num_seeds: Some(t.num_seeds),
                num_peers: Some(t.num_leechers),
            })
            .collect())
    }

    async fn edit_category(&self, name: &str, save_path: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("category", name.to_string());
        form.insert("savePath", save_path.to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/editCategory",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn create_category(&self, name: &str, save_path: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("category", name.to_string());
        form.insert("savePath", save_path.to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/createCategory",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn remove_category(&self, name: &str) -> Result<(), String> {
        let mut form = HashMap::new();
        form.insert("categories", name.to_string());
        self.request(
            reqwest::Method::POST,
            "/api/v2/torrents/removeCategories",
            None,
            Some(form),
            None,
        )
        .await?;
        Ok(())
    }

    async fn check_connection(&self) -> Result<String, String> {
        let res = self
            .request(
                reqwest::Method::GET,
                "/api/v2/app/version",
                None,
                None,
                None,
            )
            .await?;
        let version = res.text().await.map_err(|e| e.to_string())?;
        Ok(version)
    }

    async fn export_torrent(&self, hash: &str) -> Result<(Vec<u8>, String), String> {
        let torrent_info = self.get_torrent(hash, None).await?;
        let torrent_name = torrent_info
            .map(|t| t.name)
            .unwrap_or_else(|| "torrent".to_string());

        let path = format!("/api/v2/torrents/export?hash={}", hash);
        let res = self
            .request(reqwest::Method::GET, &path, None, None, None)
            .await?;
        let bytes = res.bytes().await.map_err(|e| e.to_string())?.to_vec();

        Ok((bytes, format!("{}.torrent", torrent_name)))
    }

    async fn get_speed_limit_mode(&self) -> Result<bool, String> {
        let res = self
            .request(
                reqwest::Method::GET,
                "/api/v2/transfer/speedLimitsMode",
                None,
                None,
                None,
            )
            .await?;
        let txt = res.text().await.map_err(|e| e.to_string())?;
        Ok(txt == "1")
    }

    async fn toggle_speed_limit(&self) -> Result<bool, String> {
        self.request(
            reqwest::Method::POST,
            "/api/v2/transfer/toggleSpeedLimitsMode",
            None,
            None,
            None,
        )
        .await?;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        self.get_speed_limit_mode().await
    }
}

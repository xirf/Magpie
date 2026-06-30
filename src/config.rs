use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClientSettings {
    #[serde(default = "default_client_type")]
    pub r#type: String,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_user")]
    pub user: String,
    #[serde(default = "default_password")]
    pub password: String,
}

fn default_client_type() -> String {
    "qbittorrent".to_string()
}
fn default_host() -> String {
    "http://localhost:8080".to_string()
}
fn default_user() -> String {
    "admin".to_string()
}
fn default_password() -> String {
    "adminadmin".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TelegramProxySettings {
    #[serde(default = "default_proxy_scheme")]
    pub scheme: String,
    pub hostname: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

fn default_proxy_scheme() -> String {
    "http".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TelegramSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_telegram_token")]
    pub bot_token: String,
    pub proxy: Option<TelegramProxySettings>,
}

fn default_telegram_token() -> String {
    "PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiscordSettings {
    #[serde(default = "default_false")]
    pub enabled: bool,
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UserSettings {
    #[serde(default)]
    pub user_id: i64,
    pub discord_id: Option<String>,
    #[serde(default = "default_role")]
    pub role: String,
    pub locale: Option<String>,
    #[serde(default = "default_true")]
    pub notify: bool,
    #[serde(default)]
    pub notification_filter: Vec<String>,
}

fn default_role() -> String {
    "reader".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RedisSettings {
    pub url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct S3Settings {
    #[serde(default = "default_false", alias = "enable")]
    pub enabled: bool,
    pub endpoint: Option<String>,
    #[serde(alias = "publicurl")]
    pub public_url: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
    #[serde(default = "default_link_expiry")]
    pub link_expiry: u64,
    #[serde(default = "default_s3_mode")]
    pub mode: String,
}

fn default_link_expiry() -> u64 {
    3600
}
fn default_s3_mode() -> String {
    "mount".to_string()
}

fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}

fn default_telegram_settings() -> TelegramSettings {
    TelegramSettings {
        enabled: false,
        bot_token: "".to_string(),
        proxy: None,
    }
}

fn default_discord_settings() -> DiscordSettings {
    DiscordSettings {
        enabled: false,
        token: None,
    }
}

fn default_redis_settings() -> RedisSettings {
    RedisSettings { url: None }
}

fn default_s3_settings() -> S3Settings {
    S3Settings {
        enabled: false,
        endpoint: None,
        public_url: None,
        access_key: None,
        secret_key: None,
        bucket: None,
        region: None,
        link_expiry: 3600,
        mode: "mount".to_string(),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalServerSettings {
    #[serde(default = "default_false", alias = "enable")]
    pub enabled: bool,
    #[serde(alias = "baseurl")]
    pub base_url: Option<String>,
    #[serde(alias = "bindaddr", alias = "bind_address", alias = "bindaddress")]
    pub bind_addr: Option<String>,
    #[serde(default = "default_local_link_expiry", alias = "linkexpiry")]
    pub link_expiry: u64,
}

fn default_local_link_expiry() -> u64 {
    3600
}

fn default_local_server_settings() -> LocalServerSettings {
    LocalServerSettings {
        enabled: false,
        base_url: None,
        bind_addr: None,
        link_expiry: 3600,
    }
}

/// Controls when in-progress download notifications are sent.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NotificationSettings {
    /// Edit the "added" message every N percent of progress (1–100). Default: 10.
    #[serde(default = "default_progress_interval")]
    pub progress_report_interval: u32,
    /// Minimum torrent size in GB to trigger progress reports. Default: 1.0.
    #[serde(default = "default_min_size_gb")]
    pub min_size_gb: f64,
}

fn default_progress_interval() -> u32 {
    10
}
fn default_min_size_gb() -> f64 {
    1.0
}

fn default_notification_settings() -> NotificationSettings {
    NotificationSettings {
        progress_report_interval: 10,
        min_size_gb: 1.0,
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub client: ClientSettings,
    #[serde(default = "default_telegram_settings")]
    pub telegram: TelegramSettings,
    #[serde(default = "default_discord_settings")]
    pub discord: DiscordSettings,
    #[serde(default)]
    pub users: Vec<UserSettings>,
    #[serde(default = "default_redis_settings")]
    pub redis: RedisSettings,
    #[serde(default = "default_s3_settings")]
    pub s3: S3Settings,
    #[serde(default = "default_local_server_settings")]
    pub local_server: LocalServerSettings,
    #[serde(default = "default_seed_after_download")]
    pub seed_after_download: String,
    #[serde(default = "default_notification_settings")]
    pub notifications: NotificationSettings,
}

fn default_seed_after_download() -> String {
    "always".to_string()
}

impl Settings {
    pub fn get_default_settings() -> Self {
        Self {
            client: ClientSettings {
                r#type: "qbittorrent".to_string(),
                host: "http://localhost:8080".to_string(),
                user: "admin".to_string(),
                password: "adminadmin".to_string(),
            },
            telegram: TelegramSettings {
                enabled: true,
                bot_token: "PUT_YOUR_TELEGRAM_BOT_TOKEN_HERE".to_string(),
                proxy: None,
            },
            discord: DiscordSettings {
                enabled: false,
                token: Some("PUT_YOUR_DISCORD_BOT_TOKEN_HERE".to_string()),
            },
            users: vec![UserSettings {
                user_id: 123456789,
                discord_id: None,
                role: "administrator".to_string(),
                locale: Some("en".to_string()),
                notify: true,
                notification_filter: vec![],
            }],
            redis: RedisSettings { url: None },
            s3: S3Settings {
                enabled: false,
                endpoint: None,
                public_url: None,
                access_key: None,
                secret_key: None,
                bucket: None,
                region: None,
                link_expiry: 3600,
                mode: "mount".to_string(),
            },
            local_server: LocalServerSettings {
                enabled: false,
                base_url: None,
                bind_addr: None,
                link_expiry: 3600,
            },
            seed_after_download: "always".to_string(),
            notifications: NotificationSettings {
                progress_report_interval: 10,
                min_size_gb: 1.0,
            },
        }
    }

    pub fn load_settings() -> Self {
        let data_dir = Path::new("data");
        if !data_dir.exists() {
            let _ = fs::create_dir_all(data_dir);
        }

        let yml_path = data_dir.join("config.yml");
        if !yml_path.exists() {
            let defaults = Self::get_default_settings();
            defaults.export_settings();
            return defaults;
        }

        match fs::read_to_string(&yml_path) {
            Ok(content) => match serde_yaml::from_str::<Settings>(&content) {
                Ok(settings) => settings,
                Err(e) => {
                    eprintln!("Error parsing config.yml: {:?}", e);
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("Failed to read config.yml, loading defaults: {:?}", e);
                Self::get_default_settings()
            }
        }
    }

    pub fn export_settings(&self) {
        let data_dir = Path::new("data");
        if !data_dir.exists() {
            let _ = fs::create_dir_all(data_dir);
        }

        let yml_path = data_dir.join("config.yml");
        match serde_yaml::to_string(self) {
            Ok(yaml_str) => {
                if let Ok(mut file) = File::create(yml_path) {
                    let _ = file.write_all(yaml_str.as_bytes());
                }
            }
            Err(e) => eprintln!("Failed to serialize settings to YAML: {:?}", e),
        }
    }

    #[allow(dead_code)]
    pub fn get_client_connection_string(&self) -> (String, String, String) {
        (
            self.client.host.clone(),
            self.client.user.clone(),
            self.client.password.clone(),
        )
    }

    pub fn get_proxy_connection_string(proxy: &TelegramProxySettings) -> String {
        let auth = match (&proxy.username, &proxy.password) {
            (Some(u), Some(p)) => format!("{}:{}@", u, p),
            _ => "".to_string(),
        };
        format!(
            "{}://{}{}:{}",
            proxy.scheme, auth, proxy.hostname, proxy.port
        )
    }
}

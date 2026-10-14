//! Optional Discord coding queue. Inference and coding tools are delegated to Zot.
pub mod store;
mod runner;

use serde::{Deserialize, Serialize};
use serenity::builder::{CreateAllowedMentions, CreateMessage};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub path: PathBuf,
    /// Explicitly configured verification command; never supplied by Discord or the model.
    pub check: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CodingSettings {
    pub enabled: bool,
    pub zot_binary: PathBuf,
    pub state_dir: PathBuf,
    pub api_key_env: String,
    pub model: String,
    pub base_url: String,
    pub allowed_channels: Vec<String>,
    pub projects: BTreeMap<String, Project>,
    pub max_steps: u32,
    pub timeout_seconds: u64,
    pub max_tokens: u64,
}

impl Default for CodingSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            zot_binary: PathBuf::from("zot"),
            state_dir: PathBuf::from("data/coding"),
            api_key_env: "DEEPSEEK_API_KEY".into(),
            model: "deepseek-flash".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            allowed_channels: vec![],
            projects: BTreeMap::new(),
            max_steps: 40,
            timeout_seconds: 1800,
            max_tokens: 200_000,
        }
    }
}

pub fn authorized(settings: &crate::config::Settings, discord_id: &str, channel: &str) -> bool {
    settings.coding.enabled
        && settings.coding.allowed_channels.iter().any(|id| id == channel)
        && settings.users.iter().any(|user| {
            user.discord_id.as_deref() == Some(discord_id) && user.role == "administrator"
        })
}

pub struct Service {
    pub config: CodingSettings,
    pub store: store::Store,
    active: Mutex<Option<(i64, watch::Sender<bool>)>>,
}

impl Service {
    pub fn new(mut config: CodingSettings) -> Result<Arc<Self>, String> {
        if config.allowed_channels.is_empty() || config.projects.is_empty() {
            return Err("Coding needs allowed_channels and at least one configured project".into());
        }
        if config.max_steps == 0 || config.max_steps > 200 || config.timeout_seconds == 0
            || config.timeout_seconds > 86400 || config.max_tokens == 0 {
            return Err("Invalid coding step, time, or token limit".into());
        }
        if config.model.is_empty() || !config.base_url.starts_with("https://") {
            return Err("Coding needs a model and an HTTPS DeepSeek endpoint".into());
        }
        if std::env::var(&config.api_key_env).unwrap_or_default().is_empty() {
            return Err("Configured DeepSeek key environment variable is missing".into());
        }
        std::fs::create_dir_all(&config.state_dir).map_err(|_| "Cannot create coding state directory")?;
        config.state_dir = std::fs::canonicalize(&config.state_dir).map_err(|_| "Cannot resolve coding state directory")?;
        for project in config.projects.values_mut() {
            project.path = std::fs::canonicalize(&project.path).map_err(|_| "Cannot resolve configured coding project")?;
        }
        for dir in ["worktrees", "logs", "zot", "tmp"] {
            std::fs::create_dir_all(config.state_dir.join(dir)).map_err(|_| "Cannot create coding directories")?;
        }
        let store = store::Store(config.state_dir.join("coding.db"));
        store.initialize().map_err(|_| "Cannot initialize coding queue")?;
        Ok(Arc::new(Self { config, store, active: Mutex::new(None) }))
    }

    pub async fn cancel(&self, id: i64, channel: &str) -> Result<bool, String> {
        let changed = self.store.cancel(id,channel).map_err(|_| "Cannot cancel coding task")?;
        if changed {
            let active = self.active.lock().await;
            if let Some((active_id, sender)) = active.as_ref() {
                if *active_id == id { let _ = sender.send(true); }
            }
        }
        Ok(changed)
    }

    pub fn worktree(&self, id: i64) -> PathBuf {
        self.config.state_dir.join("worktrees").join(id.to_string())
    }

    pub fn start(self: &Arc<Self>, http: Arc<Http>, settings: Arc<tokio::sync::RwLock<crate::config::Settings>>) {
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                match service.store.claim() {
                    Ok(Some(job)) => {
                        let permitted = {
                            let settings = settings.read().await;
                            authorized(&settings, &job.owner, &job.channel)
                        };
                        if !permitted {
                            let _ = service.store.finish(job.id,"failed","Task owner or channel is no longer authorized");
                            continue;
                        }
                        let (sender, receiver) = watch::channel(false);
                        {
                            let mut active = service.active.lock().await;
                            *active = Some((job.id,sender.clone()));
                            if service.store.get(job.id).ok().flatten().map(|j|j.status == "cancelled").unwrap_or(true) {
                                let _ = sender.send(true);
                            }
                        }
                        notify(&http,&job.channel,&format!("Coding task #{} started ({})",job.id,job.project)).await;
                        let result = runner::run(&service,&job,receiver).await;
                        let (status,detail) = match result {
                            Ok(verified) => (if verified { "completed" } else { "unverified" },
                                format!("Changes retained in {}. {}",service.worktree(job.id).display(),
                                    if verified { "Configured checks passed." } else { "No verification command configured." })),
                            Err(error) => ("failed",error),
                        };
                        let _ = service.store.finish(job.id,status,&detail);
                        *service.active.lock().await = None;
                        if let Ok(Some(current)) = service.store.get(job.id) {
                            notify(&http,&job.channel,&format!("Coding task #{}: {}. {}",job.id,current.status,current.detail)).await;
                        }
                    }
                    Ok(None) => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
                    Err(_) => {
                        eprintln!("Coding queue unavailable; retrying in 10 seconds");
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                }
            }
        });
    }
}

async fn notify(http: &Http, channel: &str, text: &str) {
    if let Ok(id) = channel.parse::<u64>() {
        let _ = ChannelId::new(id).send_message(http,CreateMessage::new()
            .content(text.chars().take(1800).collect::<String>())
            .allowed_mentions(CreateAllowedMentions::new().all_users(false).all_roles(false).everyone(false))).await;
    }
}

pub(super) fn local_log(path: &Path) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| "Cannot create task log".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coding_requires_explicit_administrator_and_channel() {
        let mut s = crate::config::Settings::get_default_settings();
        s.coding.enabled = true;
        s.coding.allowed_channels = vec!["123".into()];
        assert!(!authorized(&s,"999","123"));
        s.users[0].discord_id = Some("999".into());
        assert!(authorized(&s,"999","123"));
        assert!(!authorized(&s,"999","321"));
        s.users[0].role = "manager".into();
        assert!(!authorized(&s,"999","123"));
    }
    #[test]
    fn existing_config_defaults_to_coding_disabled() {
        let mut value = serde_json::to_value(crate::config::Settings::get_default_settings()).unwrap();
        value.as_object_mut().unwrap().remove("coding");
        let settings: crate::config::Settings = serde_json::from_value(value).unwrap();
        assert!(!settings.coding.enabled);
    }
}

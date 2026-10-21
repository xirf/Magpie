use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub state_dir: PathBuf,
    pub workspace: PathBuf,
    pub api_base: String,
    pub api_key_env: String,
    pub model: String,
    pub max_steps: usize,
    pub max_context_bytes: usize,
    pub max_output_tokens: u32,
    pub command_timeout_secs: u64,
    pub auto_approve: bool,
    pub telegram_user_id: Option<i64>,
    pub discord_user_id: Option<u64>,
    pub discord_channel_id: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            state_dir: ".mornye".into(),
            workspace: ".".into(),
            api_base: "https://api.deepseek.com".into(),
            api_key_env: "DEEPSEEK_API_KEY".into(),
            model: "deepseek-flash".into(),
            max_steps: 16,
            max_context_bytes: 96_000,
            max_output_tokens: 4096,
            command_timeout_secs: 120,
            auto_approve: true,
            telegram_user_id: None,
            discord_user_id: None,
            discord_channel_id: None,
        }
    }
}

impl Config {
    pub fn approval_mode(&self) -> &'static str {
        if self.auto_approve {
            "automatic: all tool actions are approved; execute without confirmation"
        } else {
            "manual: file changes, deletion, shell commands, and scheduling require /approve ID"
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let mut config: Self = if path.exists() {
            toml::from_str(&std::fs::read_to_string(path)?)?
        } else {
            Self::default()
        };
        ensure!(
            (1..=64).contains(&config.max_steps),
            "max_steps must be 1..64"
        );
        ensure!(
            (4096..=512_000).contains(&config.max_context_bytes),
            "context budget must be 4096..512000 bytes"
        );
        ensure!(
            (1..=3600).contains(&config.command_timeout_secs),
            "command timeout must be 1..3600 seconds"
        );
        ensure!(
            (1..=16384).contains(&config.max_output_tokens),
            "output token limit must be 1..16384"
        );
        let url = reqwest::Url::parse(&config.api_base)?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"))),
            "provider must use HTTPS (HTTP allowed only on loopback)"
        );
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "credentials belong in the API key environment variable"
        );
        config.workspace = std::fs::canonicalize(&config.workspace)?;
        std::fs::create_dir_all(&config.state_dir)?;
        config.state_dir = std::fs::canonicalize(&config.state_dir)?;
        Ok(config)
    }
}

use crate::{config::Config, tools};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::time::Duration;

pub struct Provider {
    client: reqwest::Client,
}

impl Provider {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(180))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    pub async fn complete(&self, config: &Config, messages: &[Value]) -> Result<Value> {
        let key = std::env::var(&config.api_key_env)
            .with_context(|| format!("Set {} before chatting", config.api_key_env))?;
        ensure!(
            !key.trim().is_empty(),
            "API key environment variable is empty"
        );
        let mut request = json!({"model":config.model,"messages":messages,"tools":tools::schemas(),
            "stream":false,"max_tokens":config.max_output_tokens});
        if reqwest::Url::parse(&config.api_base)?.host_str() == Some("api.deepseek.com") {
            request["thinking"] = json!({"type":"disabled"});
        }
        let mut response = self
            .client
            .post(format!(
                "{}/chat/completions",
                config.api_base.trim_end_matches('/')
            ))
            .bearer_auth(key)
            .json(&request)
            .send()
            .await
            .context("Provider request failed")?;
        ensure!(
            response.status().is_success(),
            "Provider returned HTTP {}. Check credentials, model, quota, or /retry later.",
            response.status()
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 1_048_576,
                "Provider response exceeds 1 MiB limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        let body: Value = serde_json::from_slice(&bytes)?;
        let choice = &body["choices"][0];
        ensure!(
            matches!(
                choice["finish_reason"].as_str(),
                Some("stop" | "tool_calls")
            ),
            "Provider response incomplete; no tools executed"
        );
        let message = choice["message"].clone();
        validate(&message)?;
        Ok(message)
    }
}

pub fn validate(message: &Value) -> Result<()> {
    ensure!(
        message["role"] == "assistant",
        "Invalid provider message role"
    );
    if let Some(calls) = message.get("tool_calls").filter(|c| !c.is_null()) {
        let calls = calls.as_array().context("tool_calls must be an array")?;
        ensure!(calls.len() <= 8, "Too many tool calls in a single response");
        let mut ids = std::collections::HashSet::new();
        for call in calls {
            let id = call["id"].as_str().context("Missing tool call ID")?;
            ensure!(
                !id.is_empty() && ids.insert(id),
                "Invalid or duplicate tool call ID"
            );
            ensure!(call["type"] == "function", "Unsupported tool type");
            ensure!(
                call["function"]["name"].is_string() && call["function"]["arguments"].is_string(),
                "Malformed tool call"
            );
        }
    }
    Ok(())
}

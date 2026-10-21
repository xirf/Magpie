use crate::agent::Agent;
use anyhow::{Context, Result, ensure};
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::time::Duration;

async fn api(client: &reqwest::Client, token: &str, method: &str, body: Value) -> Result<Value> {
    let mut response = client
        .post(format!("https://api.telegram.org/bot{token}/{method}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.without_url())?;
    ensure!(
        response.status().is_success(),
        "Telegram HTTP {}",
        response.status()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
        ensure!(
            bytes.len() + chunk.len() <= 1_048_576,
            "Telegram response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes)?;
    ensure!(body["ok"] == true, "Telegram API rejected {method}");
    Ok(body["result"].clone())
}

pub fn authorized(update: &Value, owner: i64) -> bool {
    let message = &update["message"];
    message["chat"]["type"] == "private"
        && message["chat"]["id"].as_i64() == Some(owner)
        && message["from"]["id"].as_i64() == Some(owner)
        && message["from"]["is_bot"] == false
        && message["text"].is_string()
}

async fn flush(agent: &Agent, client: &reqwest::Client, token: &str, owner: i64) -> Result<()> {
    if let Some(body) = agent
        .store
        .get("telegram_outbox")?
        .filter(|s| !s.is_empty())
    {
        let chars: Vec<char> = body.chars().collect();
        let start: usize = agent
            .store
            .get("telegram_outbox_chunk")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        for (index, chunk) in chars.chunks(1800).enumerate().skip(start) {
            api(
                client,
                token,
                "sendMessage",
                json!({"chat_id":owner,"text":chunk.iter().collect::<String>()}),
            )
            .await?;
            agent
                .store
                .set("telegram_outbox_chunk", &(index + 1).to_string())?;
        }
        agent.store.set("telegram_outbox", "")?;
        agent.store.set("telegram_outbox_chunk", "0")?;
    }
    Ok(())
}
fn queue(agent: &Agent, text: &str) -> Result<()> {
    agent.store.set("telegram_outbox_chunk", "0")?;
    agent.store.set("telegram_outbox", text)
}

pub async fn serve(agent: &Agent) -> Result<()> {
    let token = std::env::var("TELEGRAM_BOT_TOKEN").context("Set TELEGRAM_BOT_TOKEN")?;
    let owner = agent
        .config
        .telegram_user_id
        .context("Configure telegram_user_id; unknown senders are never accepted")?;
    ensure!(
        owner > 0,
        "telegram_user_id must be a positive private user ID"
    );
    let session = format!("telegram:{owner}");
    if let Some(saved) = agent.store.get("telegram_owner")? {
        ensure!(
            saved == owner.to_string(),
            "This state directory belongs to another Telegram owner. Use a separate state directory."
        );
    } else {
        agent.store.set("telegram_owner", &owner.to_string())?;
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    if agent
        .store
        .get("telegram_inflight")?
        .is_some_and(|s| !s.is_empty())
    {
        if agent
            .store
            .get("telegram_outbox")?
            .is_none_or(|s| s.is_empty())
        {
            queue(
                agent,
                "Mornye restarted during a request. Use /status to inspect evidence and /pending for approvals. Use /retry to resume or resend the message if it was not recorded. External actions were not replayed.",
            )?;
        }
        agent.store.set("telegram_inflight", "")?;
    }
    eprintln!("Mornye listening for its configured Telegram owner. Ctrl+C stops the service.");
    let mut failures = 0u32;
    loop {
        match tick(agent, &client, &token, owner, &session).await {
            Ok(()) => failures = 0,
            Err(error) => {
                failures = (failures + 1).min(5);
                eprintln!("Telegram service: {error}. Retrying with backoff.");
                tokio::time::sleep(Duration::from_secs(1u64 << failures)).await;
            }
        }
    }
}

async fn tick(
    agent: &Agent,
    client: &reqwest::Client,
    token: &str,
    owner: i64,
    session: &str,
) -> Result<()> {
    flush(agent, client, token, owner).await?;
    let offset: i64 = agent
        .store
        .get("telegram_offset")?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let updates = api(
        client,
        token,
        "getUpdates",
        json!({"offset":offset,"timeout":15,"limit":10,"allowed_updates":["message"]}),
    )
    .await?;
    for update in updates.as_array().context("Invalid Telegram update list")? {
        let id = update["update_id"].as_i64().context("Missing update ID")?;
        if id < offset {
            continue;
        }
        // Checkpoint before processing. Never replay a received /approve after a crash.
        agent.store.set("telegram_inflight", &id.to_string())?;
        agent.store.set("telegram_offset", &(id + 1).to_string())?;
        if authorized(update, owner) {
            let input = update["message"]["text"].as_str().unwrap();
            let reply = match agent.input(session, input).await {
                Ok(reply) => reply,
                Err(error) => format!(
                    "Request stopped: {error}\nProgress saved. /retry resumes; /cancel stops the turn."
                ),
            };
            queue(agent, &reply)?;
        }
        agent.store.set("telegram_inflight", "")?;
        flush(agent, client, token, owner).await?;
    }
    let idle = agent.store.latest(session)?.is_none_or(|m| {
        m["role"] == "assistant" && m["tool_calls"].as_array().is_none_or(|a| a.is_empty())
    });
    if idle && agent.store.pending(session)?.is_none() {
        let due: Option<(i64,String)> = agent.store.0.query_row("SELECT id,prompt FROM schedules WHERE session=?1 AND status='queued' AND due<=unixepoch() ORDER BY due LIMIT 1",[session],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((id, prompt)) = due {
            agent
                .store
                .0
                .execute("UPDATE schedules SET status='running' WHERE id=?1", [id])?;
            let reply = match agent
                .input(session, &format!("Scheduled task #{id}: {prompt}"))
                .await
            {
                Ok(reply) => {
                    agent
                        .store
                        .0
                        .execute("UPDATE schedules SET status='dispatched' WHERE id=?1", [id])?;
                    reply
                }
                Err(error) => {
                    agent
                        .store
                        .0
                        .execute("UPDATE schedules SET status='failed' WHERE id=?1", [id])?;
                    format!(
                        "Scheduled task #{id} stopped: {error}. Inspect /status and /retry if appropriate."
                    )
                }
            };
            queue(agent, &reply)?;
            flush(agent, client, token, owner).await?;
        }
    }
    Ok(())
}

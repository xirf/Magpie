//! Discord SDK owns gateway heartbeat/reconnect; SQLite owns admitted work.
use crate::agent::Agent;
use anyhow::{Context as _, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serenity::{
    Client,
    all::{
        ChannelId, Context, CreateAllowedMentions, CreateMessage, EventHandler, GatewayIntents,
        Http, Message,
    },
    async_trait,
};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

pub struct Queue(Connection);
impl Queue {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS inbox(id TEXT PRIMARY KEY, channel TEXT NOT NULL, body TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'queued');
            CREATE TABLE IF NOT EXISTS outbox(id INTEGER PRIMARY KEY, channel TEXT NOT NULL, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS binding(owner TEXT NOT NULL, channel TEXT NOT NULL);")?;
        Ok(Self(db))
    }
    fn bind(&self, owner: u64, channel: Option<u64>) -> Result<()> {
        let expected = (
            owner.to_string(),
            channel.map(|n| n.to_string()).unwrap_or_default(),
        );
        let saved: Option<(String, String)> = self
            .0
            .query_row("SELECT owner,channel FROM binding LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        if let Some(saved) = saved {
            ensure!(
                saved == expected,
                "Discord owner/channel changed. Use a separate state directory."
            );
        } else {
            self.0.execute(
                "INSERT INTO binding VALUES(?1,?2)",
                params![expected.0, expected.1],
            )?;
        }
        Ok(())
    }
    pub fn enqueue(&self, id: &str, channel: &str, body: &str) -> Result<bool> {
        if self.0.query_row(
            "SELECT EXISTS(SELECT 1 FROM inbox WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(false);
        }
        let count: i64 = self.0.query_row(
            "SELECT count(*) FROM inbox WHERE status IN ('queued','running')",
            [],
            |r| r.get(0),
        )?;
        ensure!(count < 64, "Request queue is full; retry later.");
        ensure!(body.len() <= 16_384, "Message exceeds input limit.");
        self.0.execute(
            "INSERT INTO inbox(id,channel,body) VALUES(?1,?2,?3)",
            params![id, channel, body],
        )?;
        Ok(true)
    }
    pub fn recover(&mut self) -> Result<()> {
        let tx = self.0.transaction()?;
        tx.execute("INSERT INTO outbox(channel,body) SELECT channel,'Mornye restarted during a request. Inspect /status and /pending before /retry. The request was not replayed.' FROM inbox WHERE status='running'", [])?;
        tx.execute(
            "UPDATE inbox SET status='interrupted' WHERE status='running'",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn claim(&mut self) -> Result<Option<(String, String, String)>> {
        let tx = self.0.transaction()?;
        let row = tx.query_row("SELECT id,channel,body FROM inbox WHERE status='queued' ORDER BY length(id),id LIMIT 1", [], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional()?;
        if let Some((id, _, _)) = &row {
            tx.execute("UPDATE inbox SET status='running' WHERE id=?1", [id])?;
        }
        tx.commit()?;
        Ok(row)
    }
    pub fn finish(&mut self, id: &str, channel: &str, reply: &str) -> Result<()> {
        let tx = self.0.transaction()?;
        for chunk in chunks(reply) {
            tx.execute(
                "INSERT INTO outbox(channel,body) VALUES(?1,?2)",
                params![channel, chunk],
            )?;
        }
        tx.execute("UPDATE inbox SET status='done',body='' WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    fn next_reply(&self) -> Result<Option<(i64, String, String)>> {
        Ok(self
            .0
            .query_row(
                "SELECT id,channel,body FROM outbox ORDER BY id LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }
}

pub fn chunks(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut chunk = String::new();
    let mut units = 0;
    for c in text.chars() {
        if units + c.len_utf16() > 1800 {
            result.push(std::mem::take(&mut chunk));
            units = 0;
        }
        chunk.push(c);
        units += c.len_utf16();
    }
    if !chunk.is_empty() {
        result.push(chunk);
    }
    if result.is_empty() {
        result.push("(No response text.)".into());
    }
    result
}
pub fn authorized(
    owner: u64,
    allowed_channel: Option<u64>,
    author: u64,
    channel: u64,
    guild: bool,
    bot: bool,
    webhook: bool,
) -> bool {
    owner == author
        && !bot
        && !webhook
        && match allowed_channel {
            Some(id) => id == channel,
            None => !guild,
        }
}
struct Handler {
    queue: Arc<Mutex<Queue>>,
    owner: u64,
    channel: Option<u64>,
}
#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, message: Message) {
        if !authorized(
            self.owner,
            self.channel,
            message.author.id.get(),
            message.channel_id.get(),
            message.guild_id.is_some(),
            message.author.bot,
            message.webhook_id.is_some(),
        ) || message.content.is_empty()
        {
            return;
        }
        let admitted = self.queue.lock().expect("queue mutex").enqueue(
            &message.id.to_string(),
            &message.channel_id.to_string(),
            &message.content,
        );
        if let Err(error) = admitted {
            let _ = message
                .channel_id
                .send_message(
                    &ctx.http,
                    reply_builder(&format!("Request not accepted: {error}")),
                )
                .await;
        }
    }
}
fn reply_builder(text: &str) -> CreateMessage {
    CreateMessage::new().content(text).allowed_mentions(
        CreateAllowedMentions::new()
            .everyone(false)
            .all_users(false)
            .all_roles(false)
            .replied_user(false),
    )
}

pub async fn serve(agent: &Agent) -> Result<()> {
    let token = std::env::var("DISCORD_BOT_TOKEN").context("Set DISCORD_BOT_TOKEN")?;
    let owner = agent
        .config
        .discord_user_id
        .filter(|n| *n > 0)
        .context("Configure discord_user_id")?;
    let mut q = Queue::open(&agent.config.state_dir.join("discord.sqlite"))?;
    q.bind(owner, agent.config.discord_channel_id)?;
    q.recover()?;
    let queue = Arc::new(Mutex::new(q));
    let http = Http::new(&token);
    let handler = Handler {
        queue: queue.clone(),
        owner,
        channel: agent.config.discord_channel_id,
    };
    let mut intents = GatewayIntents::DIRECT_MESSAGES;
    if agent.config.discord_channel_id.is_some() {
        intents |= GatewayIntents::GUILD_MESSAGES | GatewayIntents::MESSAGE_CONTENT;
    }
    // A dedicated runtime keeps blocking host reads/SQLite work off gateway heartbeats.
    let (stopped_tx, mut stopped_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(anyhow::Error::from)
            .and_then(|rt| {
                rt.block_on(async move {
                    let mut client = Client::builder(&token, intents)
                        .event_handler(handler)
                        .await?;
                    client.start().await?;
                    Ok(())
                })
            });
        let _ = stopped_tx.send(result);
    });
    eprintln!(
        "Discord gateway started. Only the configured owner and channel policy are accepted."
    );
    loop {
        match stopped_rx.try_recv() {
            Ok(result) => {
                result?;
                anyhow::bail!("Discord gateway stopped");
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                anyhow::bail!("Discord gateway thread stopped unexpectedly")
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
        }
        let pending_reply = queue.lock().expect("queue mutex").next_reply()?;
        if let Some((id, channel, body)) = pending_reply {
            match ChannelId::new(channel.parse()?)
                .send_message(&http, reply_builder(&body))
                .await
            {
                Ok(_) => {
                    queue
                        .lock()
                        .expect("queue mutex")
                        .0
                        .execute("DELETE FROM outbox WHERE id=?1", [id])?;
                }
                Err(_) => {
                    eprintln!("Discord reply delivery failed; retained for retry.");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
            continue;
        }
        let request = queue.lock().expect("queue mutex").claim()?;
        if let Some((id, channel, body)) = request {
            let session = format!("discord:{owner}:{channel}");
            let reply = match agent.input(&session, &body).await {
                Ok(reply) => reply,
                Err(error) => format!(
                    "Request stopped: {error}\nInspect /status. /retry resumes; /cancel stops the turn."
                ),
            };
            queue
                .lock()
                .expect("queue mutex")
                .finish(&id, &channel, &reply)?;
        } else {
            dispatch_schedule(agent, &queue, owner).await?;
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

async fn dispatch_schedule(agent: &Agent, queue: &Arc<Mutex<Queue>>, owner: u64) -> Result<()> {
    let prefix = format!("discord:{owner}:%");
    let due: Option<(i64,String,String)> = agent.store.0.query_row("SELECT id,session,prompt FROM schedules WHERE session LIKE ?1 AND status='queued' AND due<=unixepoch() ORDER BY due LIMIT 1", [prefix], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((id, session, prompt)) = due {
        let idle = agent.store.latest(&session)?.is_none_or(|m| {
            m["role"] == "assistant" && m["tool_calls"].as_array().is_none_or(|a| a.is_empty())
        });
        if !idle || agent.store.pending(&session)?.is_some() {
            return Ok(());
        }
        let channel = session
            .rsplit(':')
            .next()
            .context("Invalid schedule scope")?;
        // Queue id is stable across a crash between these two databases. No repeated dispatch.
        queue.lock().expect("queue mutex").enqueue(
            &format!("schedule:{id}"),
            channel,
            &format!("Scheduled task #{id}: {prompt}"),
        )?;
        agent
            .store
            .0
            .execute("UPDATE schedules SET status='dispatched' WHERE id=?1", [id])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ingress_policy_and_unicode_limits() {
        assert!(authorized(42, None, 42, 10, false, false, false));
        assert!(!authorized(42, None, 42, 10, true, false, false));
        assert!(!authorized(42, Some(10), 43, 10, true, false, false));
        assert!(!authorized(42, Some(10), 42, 11, true, false, false));
        assert!(!authorized(42, Some(10), 42, 10, true, true, false));
        assert!(!authorized(42, Some(10), 42, 10, true, false, true));
        assert!(authorized(42, Some(10), 42, 10, true, false, false));
        let text = "🙂".repeat(2500);
        let parts = chunks(&text);
        assert_eq!(parts.concat(), text);
        assert!(parts.iter().all(|p| p.encode_utf16().count() <= 1800));
    }
    #[test]
    fn durable_queue_deduplicates_and_never_replays_running_approvals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("discord.db");
        let mut q = Queue::open(&path).unwrap();
        q.bind(42, None).unwrap();
        assert!(q.bind(43, None).is_err());
        assert!(q.enqueue("123", "10", "/approve secret").unwrap());
        assert!(!q.enqueue("123", "10", "/approve secret").unwrap());
        assert!(q.claim().unwrap().is_some());
        drop(q);
        let mut q = Queue::open(&path).unwrap();
        q.recover().unwrap();
        assert!(q.claim().unwrap().is_none());
        assert!(q.next_reply().unwrap().is_some());
        assert!(!q.enqueue("123", "10", "/approve secret").unwrap());
        q.enqueue("124", "10", "hello").unwrap();
        q.claim().unwrap();
        q.finish("124", "10", "reply").unwrap();
        assert!(q.claim().unwrap().is_none());
    }
}

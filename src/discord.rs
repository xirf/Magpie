//! Discord SDK owns gateway heartbeat/reconnect; SQLite owns admitted work.
use crate::agent::Agent;
use anyhow::{Context as _, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serenity::{
    Client,
    all::{
        ChannelId, Context, CreateAllowedMentions, CreateMessage, EditMessage, EventHandler,
        GatewayIntents, Http, Message, MessageId,
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
        // Additive migration preserves existing queues and undelivered replies.
        for (table, column) in [("inbox", "progress_message"), ("outbox", "message_id")] {
            let mut stmt = db.prepare(&format!("PRAGMA table_info({table})"))?;
            let columns = stmt
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if !columns.iter().any(|c| c == column) {
                db.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"), [])?;
            }
        }
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
        tx.execute("INSERT INTO outbox(channel,body,message_id) SELECT channel,'I restarted while working on your request. I have not replayed it. Use /status and /pending to inspect it, then /retry when you are ready.',progress_message FROM inbox WHERE status='running'", [])?;
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
        let message_id: Option<String> = tx.query_row(
            "SELECT progress_message FROM inbox WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        for (index, chunk) in chunks(reply).into_iter().enumerate() {
            tx.execute(
                "INSERT INTO outbox(channel,body,message_id) VALUES(?1,?2,?3)",
                params![
                    channel,
                    chunk,
                    if index == 0 {
                        message_id.as_deref()
                    } else {
                        None
                    }
                ],
            )?;
        }
        tx.execute("UPDATE inbox SET status='done',body='' WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    fn next_reply(&self) -> Result<Option<Reply>> {
        Ok(self
            .0
            .query_row(
                "SELECT id,channel,body,message_id FROM outbox ORDER BY id LIMIT 1",
                [],
                |r| {
                    Ok(Reply {
                        id: r.get(0)?,
                        channel: r.get(1)?,
                        body: r.get(2)?,
                        message_id: r.get(3)?,
                    })
                },
            )
            .optional()?)
    }
}

struct Reply {
    id: i64,
    channel: String,
    body: String,
    message_id: Option<String>,
}
fn edit_builder(text: &str) -> EditMessage {
    EditMessage::new().content(text).allowed_mentions(
        CreateAllowedMentions::new()
            .everyone(false)
            .all_users(false)
            .all_roles(false)
            .replied_user(false),
    )
}
async fn deliver(http: &Http, reply: &Reply) -> Result<()> {
    let channel = ChannelId::new(reply.channel.parse()?);
    if let Some(id) = &reply.message_id {
        match channel
            .edit_message(http, MessageId::new(id.parse()?), edit_builder(&reply.body))
            .await
        {
            Ok(_) => return Ok(()),
            // A deleted status must not permanently block the durable reply queue.
            Err(serenity::Error::Http(error))
                if error.status_code().is_some_and(|s| s.as_u16() == 404) => {}
            Err(error) => return Err(error.into()),
        }
    }
    channel
        .send_message(http, reply_builder(&reply.body))
        .await?;
    Ok(())
}

async fn respond(
    agent: &Agent,
    queue: &Arc<Mutex<Queue>>,
    http: &Http,
    id: &str,
    channel: &str,
    session: &str,
    body: &str,
) -> Result<String> {
    let channel_id = ChannelId::new(channel.parse()?);
    let mut progress = agent.subscribe_progress();
    let status = tokio::time::timeout(
        Duration::from_secs(3),
        channel_id.send_message(http, reply_builder("I'm here. Let me think…")),
    )
    .await;
    let message_id = match status {
        Ok(Ok(message)) => {
            queue.lock().expect("queue mutex").0.execute(
                "UPDATE inbox SET progress_message=?1 WHERE id=?2",
                params![message.id.to_string(), id],
            )?;
            Some(message.id)
        }
        _ => None, // Status is best effort; it never authorizes or blocks work.
    };
    let mut last_text = "I'm here. Let me think…";
    let mut ticks = tokio::time::interval(Duration::from_secs(4));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let work = agent.input(session, body);
    tokio::pin!(work);
    loop {
        tokio::select! {
            reply = &mut work => return reply,
            _ = ticks.tick() => {
                let _ = tokio::time::timeout(Duration::from_secs(2), channel_id.broadcast_typing(http)).await;
                let text = progress.borrow_and_update().text();
                if text != last_text
                    && let Some(message_id) = message_id
                    && matches!(tokio::time::timeout(Duration::from_secs(2), channel_id.edit_message(http, message_id, edit_builder(text))).await, Ok(Ok(_)))
                {
                    last_text = text;
                }
            }
        }
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
        if let Some(reply) = pending_reply {
            match deliver(&http, &reply).await {
                Ok(_) => {
                    queue
                        .lock()
                        .expect("queue mutex")
                        .0
                        .execute("DELETE FROM outbox WHERE id=?1", [reply.id])?;
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
            let reply = match respond(agent, &queue, &http, &id, &channel, &session, &body).await {
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
    #[test]
    fn status_target_survives_restart_and_only_first_chunk_edits_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("discord.db");
        // Exercise migration from the original schema, preserving an older reply.
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE inbox(id TEXT PRIMARY KEY,channel TEXT NOT NULL,body TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'queued'); CREATE TABLE outbox(id INTEGER PRIMARY KEY,channel TEXT NOT NULL,body TEXT NOT NULL); INSERT INTO outbox(channel,body) VALUES('10','older reply');").unwrap();
        drop(db);
        let mut q = Queue::open(&path).unwrap();
        assert_eq!(q.next_reply().unwrap().unwrap().body, "older reply");
        q.0.execute("DELETE FROM outbox", []).unwrap();
        q.enqueue("123", "10", "hello").unwrap();
        q.claim().unwrap();
        q.0.execute("UPDATE inbox SET progress_message='99' WHERE id='123'", [])
            .unwrap();
        drop(q);
        let mut q = Queue::open(&path).unwrap();
        q.recover().unwrap();
        let recovered = q.next_reply().unwrap().unwrap();
        assert_eq!(recovered.message_id.as_deref(), Some("99"));
        assert!(recovered.body.contains("not replayed"));
        q.0.execute("DELETE FROM outbox", []).unwrap();
        q.enqueue("124", "10", "next").unwrap();
        q.claim().unwrap();
        q.0.execute("UPDATE inbox SET progress_message='100' WHERE id='124'", [])
            .unwrap();
        q.finish("124", "10", &"🙂".repeat(1200)).unwrap();
        let first = q.next_reply().unwrap().unwrap();
        assert_eq!(first.message_id.as_deref(), Some("100"));
        q.0.execute("DELETE FROM outbox WHERE id=?1", [first.id])
            .unwrap();
        assert!(q.next_reply().unwrap().unwrap().message_id.is_none());
        drop(q);
        let mut q = Queue::open(&path).unwrap();
        q.recover().unwrap();
        assert!(q.claim().unwrap().is_none());
    }

    struct DiscordMock {
        http: Http,
        requests: std::sync::mpsc::Receiver<(String, serde_json::Value)>,
        stop: Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl DiscordMock {
        fn new(deleted: bool) -> Self {
            use std::{
                io::{Read, Write},
                net::TcpListener,
                sync::atomic::{AtomicBool, Ordering},
            };
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let shutdown = stop.clone();
            let (tx, requests) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || {
                while !shutdown.load(Ordering::Relaxed) {
                    let (mut socket, _) = match listener.accept() {
                        Ok(pair) => pair,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(e) => panic!("{e}"),
                    };
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 4096];
                    let (route, body) = loop {
                        let n = socket.read(&mut buffer).unwrap();
                        assert!(n > 0);
                        request.extend_from_slice(&buffer[..n]);
                        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&request[..end]);
                            let len = headers
                                .lines()
                                .find_map(|l| {
                                    l.to_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            if request.len() >= end + 4 + len {
                                let body = if len == 0 {
                                    serde_json::Value::Null
                                } else {
                                    serde_json::from_slice(&request[end + 4..end + 4 + len])
                                        .unwrap()
                                };
                                break (headers.lines().next().unwrap().to_owned(), body);
                            }
                        }
                    };
                    let (status, response) = if route.contains("/typing") {
                        ("204 No Content", String::new())
                    } else if deleted && route.starts_with("PATCH") {
                        (
                            "404 Not Found",
                            r#"{"code":10008,"message":"Unknown Message"}"#.to_owned(),
                        )
                    } else {
                        let mut message = Message::default();
                        message.id = MessageId::new(99);
                        message.channel_id = ChannelId::new(10);
                        message.author.id = serenity::all::UserId::new(42);
                        message.content = body["content"].as_str().unwrap_or("").to_owned();
                        ("200 OK", serde_json::to_string(&message).unwrap())
                    };
                    tx.send((route, body)).unwrap();
                    write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
                }
            });
            Self {
                http: serenity::http::HttpBuilder::new("test-token")
                    .proxy(format!("http://{addr}"))
                    .ratelimiter_disabled(true)
                    .build(),
                requests,
                stop,
                thread: Some(thread),
            }
        }
    }
    impl Drop for DiscordMock {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn slow_tool_shows_typing_and_edits_status_into_final_reply() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let (base, _) = crate::tests::mock_captured(vec![
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"slow-tool","type":"function","function":{"name":"shell","arguments":json!({"command":"sleep 5","purpose":"Wait to exercise visible progress"}).to_string()}}]}),
            json!({"role":"assistant","content":"All done. Let's carry on."}),
        ]);
        let config = crate::config::Config {
            workspace: dir.path().to_owned(),
            state_dir: dir.path().join("state"),
            api_base: base,
            api_key_env: "PATH".into(),
            ..Default::default()
        };
        std::fs::create_dir_all(&config.state_dir).unwrap();
        let agent = Agent::new(config).unwrap();
        let mut queue = Queue::open(&dir.path().join("queue.db")).unwrap();
        queue.enqueue("123", "10", "Wait a moment").unwrap();
        queue.claim().unwrap();
        let queue = Arc::new(Mutex::new(queue));
        let discord = DiscordMock::new(false);
        let reply = respond(
            &agent,
            &queue,
            &discord.http,
            "123",
            "10",
            "discord:42:10",
            "Wait a moment",
        )
        .await
        .unwrap();
        queue.lock().unwrap().finish("123", "10", &reply).unwrap();
        let queued = queue.lock().unwrap().next_reply().unwrap().unwrap();
        deliver(&discord.http, &queued).await.unwrap();
        let calls: Vec<_> = discord.requests.try_iter().collect();
        assert!(calls.iter().filter(|(r, _)| r.contains("/typing")).count() >= 2);
        assert_eq!(
            calls
                .iter()
                .filter(|(r, _)| r.starts_with("POST") && r.contains("/messages"))
                .count(),
            1
        );
        assert!(
            calls
                .iter()
                .any(|(r, b)| r.starts_with("PATCH") && b["content"] == "I'm running a command…")
        );
        assert_eq!(calls.last().unwrap().1["content"], reply);
        for (route, body) in &calls {
            if route.contains("/messages") {
                assert_eq!(body["allowed_mentions"]["parse"], json!([]));
                assert!(!body.to_string().contains("sleep 5"));
            }
        }
    }

    #[tokio::test]
    async fn deleted_status_falls_back_to_new_reply() {
        let discord = DiscordMock::new(true);
        deliver(
            &discord.http,
            &Reply {
                id: 1,
                channel: "10".into(),
                body: "Here is your answer.".into(),
                message_id: Some("99".into()),
            },
        )
        .await
        .unwrap();
        let calls: Vec<_> = discord.requests.try_iter().collect();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].0.starts_with("PATCH"));
        assert!(calls[1].0.starts_with("POST"));
    }
}

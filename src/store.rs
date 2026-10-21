use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::path::Path;

pub struct Store(pub Connection);

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA cache_size=-2048;
            PRAGMA journal_size_limit=4194304; PRAGMA wal_autocheckpoint=1000;
            CREATE TABLE IF NOT EXISTS messages(id INTEGER PRIMARY KEY, session TEXT NOT NULL, body TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS messages_session ON messages(session,id);
            CREATE TABLE IF NOT EXISTS actions(id TEXT PRIMARY KEY, session TEXT NOT NULL, call_id TEXT NOT NULL,
                tool TEXT NOT NULL, args TEXT NOT NULL, status TEXT NOT NULL, result TEXT,
                created INTEGER NOT NULL DEFAULT(unixepoch()), UNIQUE(session,call_id));
            CREATE TABLE IF NOT EXISTS memories(id INTEGER PRIMARY KEY, session TEXT NOT NULL, body TEXT NOT NULL,
                source TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active', created INTEGER NOT NULL DEFAULT(unixepoch()));
            CREATE TABLE IF NOT EXISTS memory_feedback(session TEXT NOT NULL, memory_id INTEGER NOT NULL, signal TEXT NOT NULL CHECK(signal IN ('useful','wrong')), created INTEGER NOT NULL DEFAULT(unixepoch()), PRIMARY KEY(session,memory_id));
            CREATE TABLE IF NOT EXISTS memory_history(id INTEGER PRIMARY KEY, session TEXT NOT NULL, memory_id INTEGER NOT NULL, event TEXT NOT NULL, old_text TEXT, new_text TEXT, source_message_id INTEGER, created INTEGER NOT NULL DEFAULT(unixepoch()));
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(body, content='memories', content_rowid='id');
            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memory_fts(rowid,body) VALUES(new.id,new.body); END;
            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memory_fts(memory_fts,rowid,body) VALUES('delete',old.id,old.body); END;
            CREATE TABLE IF NOT EXISTS skills(id INTEGER PRIMARY KEY, session TEXT NOT NULL, body TEXT NOT NULL,
                evidence TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'candidate', created INTEGER NOT NULL DEFAULT(unixepoch()));
            CREATE TABLE IF NOT EXISTS kv(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS schedules(id INTEGER PRIMARY KEY, session TEXT NOT NULL, prompt TEXT NOT NULL,
                due INTEGER NOT NULL, status TEXT NOT NULL DEFAULT 'queued');")?;
        // External effects cannot be replayed safely after a crash.
        db.execute("UPDATE actions SET status='uncertain',result=?1 WHERE status='executing'",
            [json!({"status":"uncertain","message":"Process stopped during execution. Inspect actual state before retrying; this action was not replayed."}).to_string()])?;
        db.execute(
            "UPDATE schedules SET status='interrupted' WHERE status='running'",
            [],
        )?;
        // Safe additive migration for state databases created by v0.1.
        let has_status: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('memories') WHERE name='status')",
            [],
            |r| r.get(0),
        )?;
        if !has_status {
            db.execute(
                "ALTER TABLE memories ADD COLUMN status TEXT NOT NULL DEFAULT 'active'",
                [],
            )?;
            db.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", [])?;
        }
        Ok(Self(db))
    }

    pub fn append(&self, session: &str, message: &Value) -> Result<()> {
        self.0.execute(
            "INSERT INTO messages(session,body) VALUES(?1,?2)",
            params![session, message.to_string()],
        )?;
        Ok(())
    }

    pub fn history(&self, session: &str, budget: usize) -> Result<Vec<Value>> {
        // Start at a user boundary; never split a tool-call/result group.
        let mut query = self
            .0
            .prepare("SELECT body FROM messages WHERE session=?1 ORDER BY id DESC LIMIT 1024")?;
        let mut bytes = 0;
        let mut rows = Vec::new();
        let mut boundary = None;
        for row in query.query_map([session], |r| r.get::<_, String>(0))? {
            let body = row?;
            bytes += body.len();
            let value: Value = serde_json::from_str(&body)?;
            let is_user = value["role"] == "user";
            if bytes > budget && boundary.is_some() {
                break;
            }
            ensure!(
                bytes <= budget * 2,
                "Current turn exceeds context budget. Use /cancel then /new."
            );
            rows.push(value);
            if is_user {
                boundary = Some(rows.len());
            }
        }
        if let Some(len) = boundary {
            rows.truncate(len);
        }
        rows.reverse();
        Ok(rows)
    }

    pub fn latest(&self, session: &str) -> Result<Option<Value>> {
        let body: Option<String> = self
            .0
            .query_row(
                "SELECT body FROM messages WHERE session=?1 ORDER BY id DESC LIMIT 1",
                [session],
                |r| r.get(0),
            )
            .optional()?;
        body.map(|v| Ok(serde_json::from_str(&v)?)).transpose()
    }

    pub fn action(&self, session: &str, call_id: &str) -> Result<Option<Value>> {
        Ok(self.0.query_row("SELECT id,tool,args,status,result FROM actions WHERE session=?1 AND call_id=?2", params![session, call_id], |r| {
            let args: String = r.get(2)?;
            let result: Option<String> = r.get(4)?;
            Ok(json!({"id":r.get::<_,String>(0)?,"tool":r.get::<_,String>(1)?,"args":serde_json::from_str::<Value>(&args).unwrap_or(Value::Null),"status":r.get::<_,String>(3)?,"result":result}))
        }).optional()?)
    }

    pub fn pending(&self, session: &str) -> Result<Option<Value>> {
        let call: Option<String> = self.0.query_row("SELECT call_id FROM actions WHERE session=?1 AND status='pending' ORDER BY created LIMIT 1", [session], |r| r.get(0)).optional()?;
        match call {
            Some(id) => self.action(session, &id),
            None => Ok(None),
        }
    }

    pub fn finish(&self, id: &str, result: &Value) -> Result<()> {
        self.0.execute(
            "UPDATE actions SET status='finished',result=?2 WHERE id=?1",
            params![id, result.to_string()],
        )?;
        Ok(())
    }

    pub fn memories(&self, session: &str, query: &str) -> Result<Value> {
        let words: Vec<_> = query
            .split_whitespace()
            .take(12)
            .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
            .collect();
        let mut stmt = self.0.prepare("SELECT m.id,m.body,m.source,COALESCE(fb.signal,'unrated') FROM memory_fts f JOIN memories m ON m.id=f.rowid LEFT JOIN memory_feedback fb ON fb.session=m.session AND fb.memory_id=m.id WHERE memory_fts MATCH ?1 AND m.session=?2 AND m.status='active' ORDER BY rank LIMIT 8")?;
        let rows = stmt.query_map(params![words.join(" OR "),session], |r| Ok(json!({"id":r.get::<_,i64>(0)?,"memory":r.get::<_,String>(1)?,"source":r.get::<_,String>(2)?,"user_feedback":r.get::<_,String>(3)?})))?;
        if words.is_empty() {
            return Ok(json!([]));
        }
        Ok(Value::Array(rows.collect::<rusqlite::Result<Vec<_>>>()?))
    }

    pub fn correct_memory(&self, session: &str, id: i64, replacement: &str) -> Result<i64> {
        let replacement = replacement.trim();
        ensure!(
            !replacement.is_empty() && replacement.len() <= 4096,
            "Replacement must contain 1..4096 bytes"
        );
        let tx = self.0.unchecked_transaction()?;
        let (old, status): (String, String) = tx.query_row(
            "SELECT body,status FROM memories WHERE id=?1 AND session=?2",
            params![id, session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            matches!(status.as_str(), "active" | "disputed"),
            "Only an active or disputed memory can be corrected"
        );
        ensure!(old != replacement, "Replacement matches the current memory");
        // The correction is issued as a direct control command, so it does not
        // have a transcript message ID. The correction event itself is its audit record.
        let source_message: Option<i64> = None;
        tx.execute("UPDATE memories SET status='superseded' WHERE id=?1 AND session=?2 AND status='active'",params![id,session])?;
        tx.execute(
            "DELETE FROM memory_feedback WHERE session=?1 AND memory_id=?2",
            params![session, id],
        )?;
        tx.execute("INSERT INTO memory_history(session,memory_id,event,old_text,new_text,source_message_id) VALUES(?1,?2,'corrected',?3,?4,?5)",params![session,id,old,replacement,source_message])?;
        let source = json!({"kind":"explicit_user_correction","corrects_memory_id":id,"verified_by":"user_command"}).to_string();
        tx.execute(
            "INSERT INTO memories(session,body,source) VALUES(?1,?2,?3)",
            params![session, replacement, source],
        )?;
        let new_id = tx.last_insert_rowid();
        tx.execute("INSERT INTO memory_history(session,memory_id,event,old_text,new_text,source_message_id) VALUES(?1,?2,'created_from_correction',?3,?4,?5)",params![session,new_id,old,replacement,source_message])?;
        tx.commit()?;
        Ok(new_id)
    }

    pub fn rate_memory(&self, session: &str, id: i64, signal: &str) -> Result<()> {
        ensure!(
            matches!(signal, "useful" | "wrong"),
            "Feedback must be useful or wrong"
        );
        let changed = self.0.execute("INSERT INTO memory_feedback(session,memory_id,signal) SELECT ?1,?2,?3 WHERE EXISTS(SELECT 1 FROM memories WHERE id=?2 AND session=?1 AND status='active') ON CONFLICT(session,memory_id) DO UPDATE SET signal=excluded.signal,created=unixepoch()",params![session,id,signal])?;
        ensure!(
            changed == 1,
            "No active memory with that ID belongs to this conversation"
        );
        if signal == "wrong" {
            self.0.execute(
                "UPDATE memories SET status='disputed' WHERE id=?1 AND session=?2",
                params![id, session],
            )?;
            self.0.execute(
                "INSERT INTO memory_history(session,memory_id,event) VALUES(?1,?2,'marked_wrong')",
                params![session, id],
            )?;
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .query_row("SELECT value FROM kv WHERE key=?1", [key], |r| r.get(0))
            .optional()?)
    }
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.0.execute(
            "INSERT INTO kv VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

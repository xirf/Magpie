use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: i64,
    pub project: String,
    pub prompt: String,
    pub channel: String,
    pub owner: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone)]
pub struct Store(pub PathBuf);

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn job(row: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job { id: row.get(0)?, project: row.get(1)?, prompt: row.get(2)?,
        channel: row.get(3)?, owner: row.get(4)?, status: row.get(5)?, detail: row.get(6)? })
}

impl Store {
    fn connection(&self) -> rusqlite::Result<Connection> {
        let conn = Connection::open(&self.0)?;
        conn.busy_timeout(Duration::from_secs(3))?;
        Ok(conn)
    }

    pub fn initialize(&self) -> rusqlite::Result<()> {
        let conn = self.connection()?;
        conn.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS coding_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project TEXT NOT NULL, prompt TEXT NOT NULL,
                channel TEXT NOT NULL, owner TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN
                    ('queued','running','verifying','completed','unverified','failed','cancelled','interrupted')),
                detail TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS coding_queue ON coding_jobs(status,id);")?;
        conn.execute("UPDATE coding_jobs SET status='interrupted',
            detail='Magpie restarted. Review the saved worktree before submitting another task.',
            updated_at=? WHERE status IN ('running','verifying')", [now()])?;
        Ok(())
    }

    pub fn enqueue(&self, project: &str, prompt: &str, channel: &str, owner: &str) -> rusqlite::Result<i64> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pending: i64 = tx.query_row("SELECT COUNT(*) FROM coding_jobs WHERE status='queued'", [], |r| r.get(0))?;
        if pending >= 100 { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute("INSERT INTO coding_jobs(project,prompt,channel,owner,status,created_at,updated_at)
            VALUES(?,?,?,?,'queued',?,?)", params![project,prompt,channel,owner,now(),now()])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn get(&self, id: i64) -> rusqlite::Result<Option<Job>> {
        self.connection()?.query_row("SELECT id,project,prompt,channel,owner,status,detail
            FROM coding_jobs WHERE id=?", [id], job).optional()
    }

    pub fn recent(&self, channel: &str) -> rusqlite::Result<Vec<Job>> {
        let conn = self.connection()?;
        let mut stmt = conn.prepare("SELECT id,project,prompt,channel,owner,status,detail
            FROM coding_jobs WHERE channel=? ORDER BY id DESC LIMIT 10")?;
        let rows = stmt.query_map([channel], job)?;
        rows.collect()
    }

    pub fn claim(&self) -> rusqlite::Result<Option<Job>> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let next = tx.query_row("SELECT id,project,prompt,channel,owner,status,detail
            FROM coding_jobs WHERE status='queued' ORDER BY id LIMIT 1", [], job).optional()?;
        if let Some(ref next) = next {
            tx.execute("UPDATE coding_jobs SET status='running',updated_at=? WHERE id=? AND status='queued'",
                params![now(),next.id])?;
        }
        tx.commit()?;
        Ok(next.map(|mut j| { j.status = "running".into(); j }))
    }

    // Terminal states, including cancellation, cannot be overwritten by a late worker result.
    pub fn finish(&self, id: i64, status: &str, detail: &str) -> rusqlite::Result<bool> {
        Ok(self.connection()?.execute("UPDATE coding_jobs SET status=?,detail=?,updated_at=?
            WHERE id=? AND status IN ('running','verifying')", params![status,detail,now(),id])? == 1)
    }

    pub fn cancel(&self, id: i64, channel: &str) -> rusqlite::Result<bool> {
        Ok(self.connection()?.execute("UPDATE coding_jobs SET status='cancelled',detail='Cancelled by administrator',updated_at=?
            WHERE id=? AND channel=? AND status IN ('queued','running','verifying')", params![now(),id,channel])? == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        let path = std::env::temp_dir().join(format!("magpie-coding-{}-{}.db",std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        let store = Store(path);
        store.initialize().unwrap();
        store
    }

    #[test]
    fn cancellation_is_scoped_and_cannot_be_overwritten() {
        let s = store();
        let id = s.enqueue("repo","fix it","123","456").unwrap();
        assert!(!s.cancel(id,"999").unwrap());
        assert_eq!(s.claim().unwrap().unwrap().id,id);
        assert!(s.cancel(id,"123").unwrap());
        assert!(!s.finish(id,"completed","late result").unwrap());
        assert_eq!(s.get(id).unwrap().unwrap().status,"cancelled");
    }

    #[test]
    fn restart_preserves_queue_but_does_not_replay_running_jobs() {
        let s = store();
        let first = s.enqueue("repo","first","123","456").unwrap();
        let second = s.enqueue("repo","second","123","456").unwrap();
        s.claim().unwrap();
        s.initialize().unwrap();
        assert_eq!(s.get(first).unwrap().unwrap().status,"interrupted");
        assert_eq!(s.claim().unwrap().unwrap().id,second);
        assert!(s.claim().unwrap().is_none());
    }
}

use rusqlite::{params, Connection, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::config::UserSettings;

pub fn get_database_path() -> PathBuf {
    let path = if let Ok(p) = env::var("DATABASE_PATH") {
        PathBuf::from(p)
    } else {
        let data_dir = Path::new("data");
        if !data_dir.exists() {
            let _ = fs::create_dir_all(data_dir);
        }
        data_dir.join("magpie.db")
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }
    path
}

pub fn get_connection() -> Result<Connection> {
    let db_path = get_database_path();
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notifications (
            hash TEXT PRIMARY KEY,
            sent_at INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS presigned_links (
            s3_key TEXT PRIMARY KEY,
            url TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            expires_in INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            discord_id TEXT UNIQUE,
            telegram_id INTEGER UNIQUE,
            role TEXT NOT NULL,
            locale TEXT,
            notify INTEGER DEFAULT 1,
            notification_filter TEXT
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS message_torrents (
            message_id TEXT PRIMARY KEY,
            torrent_hash TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS local_downloads (
            token TEXT PRIMARY KEY,
            path TEXT NOT NULL,
            expires_at INTEGER NOT NULL
        )",
        [],
    )?;
    Ok(conn)
}

pub fn is_notification_sent(hash: &str) -> bool {
    let conn = match get_connection() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut stmt = match conn.prepare("SELECT 1 FROM notifications WHERE hash = ?") {
        Ok(s) => s,
        Err(_) => return false,
    };
    stmt.exists(params![hash]).unwrap_or(false)
}

pub fn mark_notification_sent(hash: &str) -> Result<()> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "INSERT OR REPLACE INTO notifications (hash, sent_at) VALUES (?, ?)",
        params![hash, now],
    )?;
    Ok(())
}

pub fn get_cached_presigned_url(key: &str) -> Option<String> {
    let conn = get_connection().ok()?;
    let mut stmt = conn.prepare("SELECT url, created_at, expires_in FROM presigned_links WHERE s3_key = ?").ok()?;
    
    struct RowResult {
        url: String,
        created_at: i64,
        expires_in: i64,
    }

    let mut rows = stmt.query(params![key]).ok()?;
    if let Some(row) = rows.next().ok().flatten() {
        let res = RowResult {
            url: row.get(0).ok()?,
            created_at: row.get(1).ok()?,
            expires_in: row.get(2).ok()?,
        };
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        
        let elapsed = now - res.created_at;
        let remaining = res.expires_in - elapsed;
        
        if res.expires_in > 0 {
            let percent_remaining = (remaining as f64 / res.expires_in as f64) * 100.0;
            if percent_remaining > 75.0 {
                println!(
                    "[DB] Reusing cached presigned URL for key \"{}\" ({:.1}% time remaining)",
                    key, percent_remaining
                );
                return Some(res.url);
            }
        }
    }
    None
}

pub fn cache_presigned_url(key: &str, url: &str, expires_in: i64) -> Result<()> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute(
        "INSERT OR REPLACE INTO presigned_links (s3_key, url, created_at, expires_in) VALUES (?, ?, ?, ?)",
        params![key, url, now, expires_in],
    )?;
    Ok(())
}

pub fn load_users_from_db() -> Result<Vec<UserSettings>> {
    let conn = get_connection()?;
    let mut stmt = conn.prepare("SELECT telegram_id, discord_id, role, locale, notify, notification_filter FROM users")?;
    let user_iter = stmt.query_map([], |row| {
        let telegram_id: Option<i64> = row.get(0)?;
        let discord_id: Option<String> = row.get(1)?;
        let role: String = row.get(2)?;
        let locale: Option<String> = row.get(3)?;
        let notify_int: i32 = row.get(4)?;
        let filter_str: Option<String> = row.get(5)?;
        
        let notification_filter = filter_str
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default();
        
        Ok(UserSettings {
            user_id: telegram_id.unwrap_or(0),
            discord_id,
            role,
            locale,
            notify: notify_int == 1,
            notification_filter,
        })
    })?;
    
    let mut users = Vec::new();
    for user in user_iter {
        if let Ok(u) = user {
            users.push(u);
        }
    }
    Ok(users)
}

pub fn save_user_to_db(user: &UserSettings) -> Result<()> {
    let conn = get_connection()?;
    let filter_str = serde_json::to_string(&user.notification_filter).unwrap_or_else(|_| "[]".to_string());
    let t_id = if user.user_id != 0 { Some(user.user_id) } else { None };
    conn.execute(
        "INSERT OR REPLACE INTO users (discord_id, telegram_id, role, locale, notify, notification_filter)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![
            user.discord_id,
            t_id,
            user.role,
            user.locale,
            if user.notify { 1 } else { 0 },
            filter_str
        ],
    )?;
    Ok(())
}

pub fn sync_users(yaml_users: &[UserSettings]) -> Result<Vec<UserSettings>> {
    let existing_users = load_users_from_db().unwrap_or_default();
    
    for yu in yaml_users {
        let exists = existing_users.iter().any(|eu| {
            (yu.discord_id.is_some() && yu.discord_id == eu.discord_id)
                || (yu.user_id != 0 && yu.user_id == eu.user_id)
        });
        
        if !exists {
            let _ = save_user_to_db(yu);
        }
    }
    
    load_users_from_db()
}

pub fn associate_message_with_torrent(message_id: &str, torrent_hash: &str) -> Result<()> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT OR REPLACE INTO message_torrents (message_id, torrent_hash) VALUES (?, ?)",
        params![message_id, torrent_hash],
    )?;
    Ok(())
}

pub fn get_torrent_hash_for_message(message_id: &str) -> Result<Option<String>> {
    let conn = get_connection()?;
    let mut stmt = conn.prepare("SELECT torrent_hash FROM message_torrents WHERE message_id = ?")?;
    let mut rows = stmt.query(params![message_id])?;
    if let Some(row) = rows.next()? {
        let hash: String = row.get(0)?;
        Ok(Some(hash))
    } else {
        Ok(None)
    }
}

pub fn cache_local_download(token: &str, path: &str, expires_at: i64) -> Result<()> {
    let conn = get_connection()?;
    conn.execute(
        "INSERT OR REPLACE INTO local_downloads (token, path, expires_at) VALUES (?, ?, ?)",
        params![token, path, expires_at],
    )?;
    Ok(())
}

pub fn get_local_download_path(token: &str) -> Option<String> {
    let conn = get_connection().ok()?;
    let mut stmt = conn.prepare("SELECT path, expires_at FROM local_downloads WHERE token = ?").ok()?;
    
    struct LocalDownload {
        path: String,
        expires_at: i64,
    }
    
    let mut rows = stmt.query(params![token]).ok()?;
    if let Some(row) = rows.next().ok().flatten() {
        let res = LocalDownload {
            path: row.get(0).ok()?,
            expires_at: row.get(1).ok()?,
        };
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
            
        if res.expires_at > now {
            return Some(res.path);
        }
    }
    None
}

pub fn prune_expired_local_downloads() -> Result<()> {
    let conn = get_connection()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    conn.execute("DELETE FROM local_downloads WHERE expires_at <= ?", params![now])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    
    // We use a Mutex to prevent parallel tests from running concurrently and dirtying env vars
    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn setup_test_db(test_name: &str) {
        let db_file = format!("data/test_{}.db", test_name);
        let path = Path::new(&db_file);
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        std::env::set_var("DATABASE_PATH", &db_file);
        // Trigger table creation
        let _ = get_connection().unwrap();
    }

    fn cleanup_test_db(test_name: &str) {
        let db_file = format!("data/test_{}.db", test_name);
        let path = Path::new(&db_file);
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn test_db_notifications() {
        let _lock = TEST_MUTEX.lock().unwrap();
        let name = "notifications";
        setup_test_db(name);

        assert!(!is_notification_sent("hash-1"));
        mark_notification_sent("hash-1").unwrap();
        assert!(is_notification_sent("hash-1"));
        assert!(!is_notification_sent("hash-2"));

        cleanup_test_db(name);
    }

    #[test]
    fn test_db_presigned_url_cache() {
        let _lock = TEST_MUTEX.lock().unwrap();
        let name = "presigned";
        setup_test_db(name);
        
        let key = "folder/video.mp4";
        let url = "http://test-url.com/video.mp4";
        
        // Cache with 100 seconds expiry
        cache_presigned_url(key, url, 100).unwrap();

        // Should reuse immediately (100% time remaining)
        assert_eq!(get_cached_presigned_url(key), Some(url.to_string()));

        // Test time remaining logic:
        // Since we can't easily mock std::time::SystemTime in pure std Rust without features,
        // we can verify it returns Some when initially cached, and expires appropriately if we cache with 0 expiry.
        cache_presigned_url(key, url, 0).unwrap();
        assert_eq!(get_cached_presigned_url(key), None);

        cleanup_test_db(name);
    }

    #[test]
    fn test_db_sync_and_save_users() {
        let _lock = TEST_MUTEX.lock().unwrap();
        let name = "users";
        setup_test_db(name);

        let yaml_users = vec![
            UserSettings {
                user_id: 111,
                discord_id: Some("discord-111".to_string()),
                role: "administrator".to_string(),
                locale: Some("es".to_string()),
                notify: true,
                notification_filter: vec![],
            },
            UserSettings {
                user_id: 222,
                discord_id: None,
                role: "manager".to_string(),
                locale: Some("en".to_string()),
                notify: false,
                notification_filter: vec!["movies".to_string()],
            }
        ];

        let synced = sync_users(&yaml_users).unwrap();
        assert_eq!(synced.length_matches(2), true); // Or synced.len() == 2
        
        assert_eq!(synced.len(), 2);

        let u1 = synced.iter().find(|u| u.user_id == 111).unwrap();
        assert_eq!(u1.role, "administrator");
        assert_eq!(u1.locale.as_deref(), Some("es"));
        assert_eq!(u1.notify, true);

        let u2 = synced.iter().find(|u| u.user_id == 222).unwrap();
        assert_eq!(u2.discord_id, None);
        assert_eq!(u2.role, "manager");
        assert_eq!(u2.notify, false);
        assert_eq!(u2.notification_filter, vec!["movies".to_string()]);

        // Dynamic update test
        let mut u2_mod = u2.clone();
        u2_mod.role = "administrator".to_string();
        save_user_to_db(&u2_mod).unwrap();

        let updated_users = load_users_from_db().unwrap();
        let updated_u2 = updated_users.iter().find(|u| u.user_id == 222).unwrap();
        assert_eq!(updated_u2.role, "administrator");

        cleanup_test_db(name);
    }

    #[test]
    fn test_db_local_downloads() {
        let _lock = TEST_MUTEX.lock().unwrap();
        let name = "local_downloads";
        setup_test_db(name);

        let token = "my-test-token";
        let path = "/downloads/MyTorrent";
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64 + 100;

        cache_local_download(token, path, expires_at).unwrap();
        assert_eq!(get_local_download_path(token), Some(path.to_string()));

        // Test expired token
        let expired_expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64 - 100;
        cache_local_download("expired-token", path, expired_expires_at).unwrap();
        assert_eq!(get_local_download_path("expired-token"), None);

        // Test prune
        prune_expired_local_downloads().unwrap();
        
        let conn = get_connection().unwrap();
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM local_downloads WHERE token = 'expired-token'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 0);

        cleanup_test_db(name);
    }

    trait LengthHelper {
        fn length_matches(&self, val: usize) -> bool;
    }
    impl<T> LengthHelper for Vec<T> {
        fn length_matches(&self, val: usize) -> bool {
            self.len() == val
        }
    }
}


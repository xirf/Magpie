use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use redis::{AsyncCommands, Client};

#[derive(Clone)]
struct EmulatorEntry {
    value: String,
    expires_at: Option<Instant>,
}

#[derive(Clone)]
pub struct RedisEmulator {
    storage: Arc<Mutex<HashMap<String, EmulatorEntry>>>,
}

impl RedisEmulator {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let mut storage = self.storage.lock().unwrap();
        if let Some(entry) = storage.get(key) {
            if let Some(exp) = entry.expires_at {
                if Instant::now() > exp {
                    storage.remove(key);
                    return None;
                }
            }
            return Some(entry.value.clone());
        }
        None
    }

    pub fn set(&self, key: &str, value: &str, ex: Option<u64>) {
        let expires_at = ex.map(|sec| {
            // Cap at 1 day
            let capped = sec.min(86400);
            Instant::now() + Duration::from_secs(capped)
        });
        let entry = EmulatorEntry {
            value: value.to_string(),
            expires_at,
        };
        self.storage.lock().unwrap().insert(key.to_string(), entry);
    }

    #[allow(dead_code)]
    pub fn delete(&self, key: &str) {
        self.storage.lock().unwrap().remove(key);
    }

    pub fn exists(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

pub enum RedisBackend {
    Real(redis::aio::MultiplexedConnection),
    Emulator(RedisEmulator),
}

#[derive(Clone)]
pub struct RedisWrapper {
    backend: Arc<Mutex<Option<RedisBackend>>>,
}

impl RedisWrapper {
    pub fn new() -> Self {
        Self {
            backend: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn connect(&self, url: Option<&str>) {
        if let Some(redis_url) = url {
            match Client::open(redis_url) {
                Ok(client) => match client.get_multiplexed_tokio_connection().await {
                    Ok(conn) => {
                        println!("Connected to Redis successfully");
                        *self.backend.lock().unwrap() = Some(RedisBackend::Real(conn));
                        return;
                    }
                    Err(e) => {
                        eprintln!("Redis connection failed ({:?}), falling back to in-memory storage", e);
                    }
                },
                Err(e) => {
                    eprintln!("Redis client open failed ({:?}), falling back to in-memory storage", e);
                }
            }
        } else {
            println!("Redis URL not configured. Using in-memory storage");
        }
        
        *self.backend.lock().unwrap() = Some(RedisBackend::Emulator(RedisEmulator::new()));
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        let opt_backend = {
            let mut backend_lock = self.backend.lock().unwrap();
            backend_lock.as_mut().map(|b| match b {
                RedisBackend::Real(conn) => Ok(conn.clone()),
                RedisBackend::Emulator(emu) => Err(emu.clone()),
            })
        };
        match opt_backend? {
            Ok(mut conn) => {
                let val: Option<String> = conn.get(key).await.ok().flatten();
                val
            }
            Err(emu) => emu.get(key),
        }
    }

    pub async fn set(&self, key: &str, value: &str, ex: Option<u64>) {
        let opt_backend = {
            let mut backend_lock = self.backend.lock().unwrap();
            backend_lock.as_mut().map(|b| match b {
                RedisBackend::Real(conn) => Ok(conn.clone()),
                RedisBackend::Emulator(emu) => Err(emu.clone()),
            })
        };
        if let Some(backend) = opt_backend {
            match backend {
                Ok(mut conn) => {
                    if let Some(expiry) = ex {
                        let _: Result<(), _> = conn.set_ex(key, value, expiry).await;
                    } else {
                        let _: Result<(), _> = conn.set(key, value).await;
                    }
                }
                Err(emu) => {
                    emu.set(key, value, ex);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub async fn delete(&self, key: &str) {
        let opt_backend = {
            let mut backend_lock = self.backend.lock().unwrap();
            backend_lock.as_mut().map(|b| match b {
                RedisBackend::Real(conn) => Ok(conn.clone()),
                RedisBackend::Emulator(emu) => Err(emu.clone()),
            })
        };
        if let Some(backend) = opt_backend {
            match backend {
                Ok(mut conn) => {
                    let _: Result<(), _> = conn.del(key).await;
                }
                Err(emu) => {
                    emu.delete(key);
                }
            }
        }
    }

    pub async fn exists(&self, key: &str) -> bool {
        let opt_backend = {
            let mut backend_lock = self.backend.lock().unwrap();
            backend_lock.as_mut().map(|b| match b {
                RedisBackend::Real(conn) => Ok(conn.clone()),
                RedisBackend::Emulator(emu) => Err(emu.clone()),
            })
        };
        if let Some(backend) = opt_backend {
            match backend {
                Ok(mut conn) => {
                    let res: Option<i32> = conn.exists(key).await.ok();
                    res.unwrap_or(0) > 0
                }
                Err(emu) => {
                    emu.exists(key)
                }
            }
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_redis_emulator_basic() {
        let emu = RedisEmulator::new();
        assert!(!emu.exists("mykey"));
        assert_eq!(emu.get("mykey"), None);

        emu.set("mykey", "value1", None);
        assert!(emu.exists("mykey"));
        assert_eq!(emu.get("mykey"), Some("value1".to_string()));
    }

    #[tokio::test]
    async fn test_redis_emulator_delete() {
        let emu = RedisEmulator::new();
        emu.set("mykey", "val", None);
        assert!(emu.exists("mykey"));

        emu.delete("mykey");
        assert!(!emu.exists("mykey"));
        assert_eq!(emu.get("mykey"), None);
    }

    #[tokio::test]
    async fn test_redis_emulator_expiry() {
        let emu = RedisEmulator::new();
        emu.set("mykey", "val", Some(1));
        assert!(emu.exists("mykey"));

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(!emu.exists("mykey"));
        assert_eq!(emu.get("mykey"), None);
    }

    #[tokio::test]
    async fn test_redis_emulator_overwrite_clears_expiry() {
        let emu = RedisEmulator::new();
        emu.set("mykey", "val1", Some(1));
        emu.set("mykey", "val2", None);

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(emu.exists("mykey"));
        assert_eq!(emu.get("mykey"), Some("val2".to_string()));
    }
}


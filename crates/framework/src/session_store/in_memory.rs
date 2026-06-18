use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rust_agent_core::{AgentSession, ISession, ISessionStore, Result, SessionTTLOptions};

/// 基于 `HashMap` 的内存会话存储。
///
/// 进程退出时会话丢失。适用于开发、测试和短期应用。
///
/// ## TTL 清理
///
/// 使用 `with_ttl()` 构造时，`cleanup_expired()` 将驱逐超过 `max_idle_secs` 或 `max_lifetime_secs` 的会话。
pub struct InMemorySessionStore {
    sessions: RwLock<HashMap<String, Arc<dyn ISession>>>,
    ttl: Option<SessionTTLOptions>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            ttl: None,
        }
    }

    /// Enable TTL-based session cleanup.
    pub fn with_ttl(mut self, ttl: SessionTTLOptions) -> Self {
        self.ttl = Some(ttl);
        self
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ISessionStore for InMemorySessionStore {
    async fn save_session(&self, session: &dyn ISession) -> Result<()> {
        let serialized = session.serialize()?;
        let restored = AgentSession::deserialize(&serialized)?;
        let id = session.session_id().to_string();
        self.sessions.write().await.insert(id, Arc::new(restored));
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<Arc<dyn ISession>>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned())
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.sessions.write().await.remove(session_id);
        Ok(())
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        let ttl = match &self.ttl {
            Some(t) => t,
            None => return Ok(0),
        };

        let now = chrono::Utc::now();
        let mut sessions = self.sessions.write().await;
        let mut to_remove = Vec::new();

        for (id, session) in sessions.iter() {
            let last_active = session.last_active_at();
            let created = session.created_at();

            // Check idle timeout
            if let Some(max_idle) = ttl.max_idle_secs {
                let idle_duration = now - last_active;
                if idle_duration.num_seconds() > max_idle as i64 {
                    to_remove.push(id.clone());
                    continue;
                }
            }

            // Check lifetime timeout
            if let Some(max_lifetime) = ttl.max_lifetime_secs {
                let lifetime_duration = now - created;
                if lifetime_duration.num_seconds() > max_lifetime as i64 {
                    to_remove.push(id.clone());
                }
            }
        }

        let removed = to_remove.len();
        for id in to_remove {
            sessions.remove(&id);
        }

        if removed > 0 {
            tracing::info!(removed, "Expired session eviction completed");
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::AgentSession;

    #[tokio::test]
    async fn test_cleanup_expired_no_ttl_configured() {
        let store = InMemorySessionStore::new();
        let session = Arc::new(AgentSession::with_id("s1"));
        store.save_session(session.as_ref()).await.unwrap();

        // No TTL configured — nothing should be removed
        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 0);
        assert!(store.get_session("s1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_cleanup_expired_idle_timeout() {
        let ttl = SessionTTLOptions {
            max_idle_secs: Some(1),
            max_lifetime_secs: None,
            cleanup_interval_secs: 60,
        };
        let store = InMemorySessionStore::new().with_ttl(ttl);

        // Create 3 sessions — all saved to store immediately
        let s1 = Arc::new(AgentSession::with_id("active"));
        let s2 = Arc::new(AgentSession::with_id("idle-1"));
        let s3 = Arc::new(AgentSession::with_id("idle-2"));
        store.save_session(s1.as_ref()).await.unwrap();
        store.save_session(s2.as_ref()).await.unwrap();
        store.save_session(s3.as_ref()).await.unwrap();

        // Wait for idle timeout to expire (all sessions become idle)
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Touch only s1 right before cleanup to keep it alive
        // Need to reload from store first since stored copy has its own timestamp
        let s1_reloaded = store.get_session("active").await.unwrap().unwrap();
        s1_reloaded.touch_last_active().await;
        store.save_session(s1_reloaded.as_ref()).await.unwrap();

        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 2, "Should evict 2 idle sessions");

        // s1 should remain (was touched right before cleanup)
        assert!(store.get_session("active").await.unwrap().is_some());
        // s2 and s3 should be gone
        assert!(store.get_session("idle-1").await.unwrap().is_none());
        assert!(store.get_session("idle-2").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cleanup_expired_lifetime_timeout() {
        let ttl = SessionTTLOptions {
            max_idle_secs: None,
            max_lifetime_secs: Some(1),
            cleanup_interval_secs: 60,
        };
        let store = InMemorySessionStore::new().with_ttl(ttl);

        let s1 = Arc::new(AgentSession::with_id("old"));
        store.save_session(s1.as_ref()).await.unwrap();

        // Wait for lifetime to expire
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 1, "Should evict session exceeding max_lifetime");

        assert!(store.get_session("old").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cleanup_expired_concurrent_sessions() {
        let ttl = SessionTTLOptions {
            max_idle_secs: Some(1),
            max_lifetime_secs: None,
            cleanup_interval_secs: 60,
        };
        let store = Arc::new(InMemorySessionStore::new().with_ttl(ttl));

        // Concurrent creation — all sessions saved immediately
        let mut handles = Vec::new();
        for i in 0..10 {
            let store = store.clone();
            handles.push(tokio::spawn(async move {
                let sid = format!("session-{}", i);
                let session = Arc::new(AgentSession::with_id(&sid));
                store.save_session(session.as_ref()).await.unwrap();
                sid
            }));
        }

        let mut session_ids = Vec::new();
        for h in handles {
            session_ids.push(h.await.unwrap());
        }

        // Wait for idle timeout
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Touch active sessions right before cleanup
        for i in 0..10 {
            if i % 3 == 0 {
                let sid = &session_ids[i];
                let s = store.get_session(sid).await.unwrap().unwrap();
                s.touch_last_active().await;
                store.save_session(s.as_ref()).await.unwrap();
            }
        }

        let removed = store.cleanup_expired().await.unwrap();
        // 10 total, 4 active (indices 0,3,6,9), 6 idle → 6 removed
        assert_eq!(removed, 6, "Should evict 6 idle sessions out of 10 (4 kept active)");

        for (i, sid) in session_ids.iter().enumerate() {
            if i % 3 == 0 {
                assert!(store.get_session(sid).await.unwrap().is_some(), "Active session {} should remain", sid);
            } else {
                assert!(store.get_session(sid).await.unwrap().is_none(), "Idle session {} should be removed", sid);
            }
        }
    }

    #[tokio::test]
    async fn test_cleanup_expired_already_deleted() {
        let ttl = SessionTTLOptions {
            max_idle_secs: Some(1),
            max_lifetime_secs: None,
            cleanup_interval_secs: 60,
        };
        let store = InMemorySessionStore::new().with_ttl(ttl);

        let s = Arc::new(AgentSession::with_id("s"));
        store.save_session(s.as_ref()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Delete manually first
        store.delete_session("s").await.unwrap();

        // cleanup should not panic on already-deleted sessions
        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 0, "Already deleted session should not be double-counted");
    }
}

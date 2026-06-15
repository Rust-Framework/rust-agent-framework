use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rust_agent_core::{AgentSession, ISession, ISessionStore, Result};

/// In-memory session store backed by a `HashMap`.
///
/// Sessions are lost when the process exits.
/// Suitable for development, testing, and short-lived applications.
pub struct InMemorySessionStore {
    sessions: RwLock<HashMap<String, Arc<dyn ISession>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
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
        // In-memory store doesn't support TTL-based cleanup by default.
        // Sessions persist until explicitly deleted or the process exits.
        Ok(0)
    }
}

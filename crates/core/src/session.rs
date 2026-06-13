use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use crate::{ChatMessage, Result};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Session interface following MAF's thread/session management.
#[async_trait]
pub trait ISession: Send + Sync {
    fn session_id(&self) -> &str;
    async fn add_message(&self, message: ChatMessage) -> Result<()>;
    async fn get_messages(&self) -> Result<Vec<ChatMessage>>;
    async fn clear(&self) -> Result<()>;
}

/// AgentSession — default ISession implementation following MAF's thread model.
pub struct AgentSession {
    session_id: String,
    history: RwLock<Vec<ChatMessage>>,
}

impl AgentSession {
    pub fn new() -> Self {
        let id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self { session_id: format!("session-{}", id), history: RwLock::new(Vec::new()) }
    }

    pub fn with_id(session_id: impl Into<String>) -> Self {
        Self { session_id: session_id.into(), history: RwLock::new(Vec::new()) }
    }
}

impl Default for AgentSession {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl ISession for AgentSession {
    fn session_id(&self) -> &str { &self.session_id }

    async fn add_message(&self, message: ChatMessage) -> Result<()> {
        self.history.write().await.push(message);
        Ok(())
    }

    async fn get_messages(&self) -> Result<Vec<ChatMessage>> {
        Ok(self.history.read().await.clone())
    }

    async fn clear(&self) -> Result<()> {
        self.history.write().await.clear();
        Ok(())
    }
}

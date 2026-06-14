use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{ChatMessage, Result, AgentError};

/// Session — MAF AgentSession equivalent, manages multi-turn message lifecycle.
#[async_trait]
pub trait ISession: Send + Sync {
    fn session_id(&self) -> &str;
    async fn add_message(&self, message: ChatMessage) -> Result<()>;
    async fn get_messages(&self) -> Result<Vec<ChatMessage>>;
    async fn clear(&self) -> Result<()>;
    fn metadata(&self) -> SessionMetadata;
    fn snapshot(&self) -> SessionSnapshot;
    fn serialize(&self) -> Result<String>;
    fn deserialize(data: &str) -> Result<Self> where Self: Sized;
}

/// Session metadata — creation time, message count, cache tracking hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u64,
    /// Hash of the last request's full messages array, used for KV cache prefix tracking
    pub last_request_hash: Option<u64>,
}

/// Read-only session snapshot for debugging, UI display, auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub metadata: SessionMetadata,
    pub messages: Vec<ChatMessage>,
}

/// Default in-memory session implementation
pub struct AgentSession {
    session_id: String,
    history: RwLock<Vec<ChatMessage>>,
    metadata: RwLock<SessionMetadata>,
}

impl AgentSession {
    /// Create a new session with a UUID-based session_id (no global counter)
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            session_id: Uuid::new_v4().to_string(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: now,
                updated_at: now,
                message_count: 0,
                last_request_hash: None,
            }),
        }
    }

    /// Create a session with a specific session_id (for restoring persisted sessions)
    pub fn with_id(session_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            session_id: session_id.into(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: now,
                updated_at: now,
                message_count: 0,
                last_request_hash: None,
            }),
        }
    }

    /// Record the hash of the current request's full messages for cache tracking.
    /// Called by HistoryAgent before sending to LLM.
    pub fn touch_request_hash(&self, messages: &[ChatMessage]) {
        let hash = hash_messages(messages);
        let mut meta = self.metadata.blocking_write();
        meta.last_request_hash = Some(hash);
    }
}

impl Default for AgentSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple xxhash64 for message content fingerprinting
fn hash_messages(messages: &[ChatMessage]) -> u64 {
    // Use a simple hash based on role + content concatenation
    use std::hash::{Hash, Hasher};
    // Use std's DefaultHasher — it's not xxhash but it's in std, zero deps
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for msg in messages {
        std::mem::discriminant(&msg.role).hash(&mut hasher);
        msg.content.hash(&mut hasher);
    }
    hasher.finish()
}

#[async_trait]
impl ISession for AgentSession {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    async fn add_message(&self, message: ChatMessage) -> Result<()> {
        self.history.write().await.push(message);
        let mut meta = self.metadata.write().await;
        meta.message_count += 1;
        meta.updated_at = Utc::now();
        Ok(())
    }

    async fn get_messages(&self) -> Result<Vec<ChatMessage>> {
        Ok(self.history.read().await.clone())
    }

    async fn clear(&self) -> Result<()> {
        self.history.write().await.clear();
        let mut meta = self.metadata.write().await;
        meta.message_count = 0;
        meta.updated_at = Utc::now();
        Ok(())
    }

    fn metadata(&self) -> SessionMetadata {
        let meta = self.metadata.blocking_read();
        meta.clone()
    }

    fn snapshot(&self) -> SessionSnapshot {
        let history = self.history.blocking_read();
        let meta = self.metadata.blocking_read();
        SessionSnapshot {
            session_id: self.session_id.clone(),
            metadata: SessionMetadata {
                created_at: meta.created_at,
                updated_at: meta.updated_at,
                message_count: meta.message_count,
                last_request_hash: meta.last_request_hash,
            },
            messages: history.clone(),
        }
    }

    fn serialize(&self) -> Result<String> {
        serde_json::to_string(&self.snapshot())
            .map_err(|e| AgentError::Serialize(e.to_string()))
    }

    fn deserialize(data: &str) -> Result<Self> {
        let snap: SessionSnapshot = serde_json::from_str(data)
            .map_err(|e| AgentError::Serialize(e.to_string()))?;
        Ok(Self {
            session_id: snap.session_id,
            history: RwLock::new(snap.messages),
            metadata: RwLock::new(snap.metadata),
        })
    }
}

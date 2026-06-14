use std::collections::HashMap;

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

    /// 获取指定 Provider 在本次 Session 中存储的状态
    fn get_provider_state(&self, _provider_name: &str) -> Result<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }

    /// 设置指定 Provider 在本次 Session 中的状态
    fn set_provider_state(&self, _provider_name: &str, _state: serde_json::Value) -> Result<()> {
        Ok(())
    }

    /// 原子批量追加消息
    async fn add_messages_batch(&self, messages: &[ChatMessage]) -> Result<()> {
        for msg in messages {
            self.add_message(msg.clone()).await?;
        }
        Ok(())
    }

    /// 获取当前消息数量（零克隆，O(1)）
    fn get_message_count(&self) -> usize { 0 }

    /// 记录本次请求的 messages 哈希（KV cache 前缀追踪）
    fn touch_request_hash(&self, _messages: &[ChatMessage]) {}

    /// 获取上次请求的 messages 哈希
    fn get_last_request_hash(&self) -> Option<u64> { None }
}

/// Session metadata — creation time, message count, cache tracking hash
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u64,
    /// Hash of the last request's full messages array, used for KV cache prefix tracking
    pub last_request_hash: Option<u64>,
}

/// Provider 级 Session 状态存储
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderStateStore {
    states: HashMap<String, serde_json::Value>,
}

impl ProviderStateStore {
    pub fn new() -> Self { Self { states: HashMap::new() } }
    pub fn get(&self, provider_name: &str) -> Option<&serde_json::Value> {
        self.states.get(provider_name)
    }
    pub fn set(&mut self, provider_name: &str, state: serde_json::Value) {
        self.states.insert(provider_name.to_string(), state);
    }
    pub fn remove(&mut self, provider_name: &str) { self.states.remove(provider_name); }
}

/// Read-only session snapshot for debugging, UI display, auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub metadata: SessionMetadata,
    pub messages: Vec<ChatMessage>,
    pub provider_states: ProviderStateStore,
}

/// Default in-memory session implementation
pub struct AgentSession {
    session_id: String,
    history: RwLock<Vec<ChatMessage>>,
    metadata: RwLock<SessionMetadata>,
    provider_states: RwLock<ProviderStateStore>,
}

impl AgentSession {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            session_id: Uuid::new_v4().to_string(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: now, updated_at: now, message_count: 0, last_request_hash: None,
            }),
            provider_states: RwLock::new(ProviderStateStore::new()),
        }
    }

    pub fn with_id(session_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            session_id: session_id.into(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: now, updated_at: now, message_count: 0, last_request_hash: None,
            }),
            provider_states: RwLock::new(ProviderStateStore::new()),
        }
    }

    pub fn touch_request_hash(&self, messages: &[ChatMessage]) {
        let hash = hash_messages(messages);
        if let Ok(mut meta) = self.metadata.try_write() {
            meta.last_request_hash = Some(hash);
        }
    }
}

impl Default for AgentSession {
    fn default() -> Self { Self::new() }
}

fn hash_messages(messages: &[ChatMessage]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for msg in messages {
        std::mem::discriminant(&msg.role).hash(&mut hasher);
        msg.content.hash(&mut hasher);
        if let Some(ref tc) = msg.tool_calls {
            for call in tc {
                call.id.hash(&mut hasher);
                call.name.hash(&mut hasher);
            }
        }
        if let Some(ref tcid) = msg.tool_call_id {
            tcid.hash(&mut hasher);
        }
    }
    hasher.finish()
}

#[async_trait]
impl ISession for AgentSession {
    fn session_id(&self) -> &str { &self.session_id }

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
        self.metadata.try_read().map(|m| m.clone()).unwrap_or_default()
    }

    fn snapshot(&self) -> SessionSnapshot {
        let history = self.history.try_read().map(|h| h.clone()).unwrap_or_default();
        let meta = self.metadata.try_read().map(|m| m.clone()).unwrap_or_default();
        let ps = self.provider_states.try_read().map(|p| p.clone()).unwrap_or_default();
        SessionSnapshot {
            session_id: self.session_id.clone(),
            metadata: SessionMetadata {
                created_at: meta.created_at,
                updated_at: meta.updated_at,
                message_count: meta.message_count,
                last_request_hash: meta.last_request_hash,
            },
            messages: history,
            provider_states: ps,
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
            provider_states: RwLock::new(snap.provider_states),
        })
    }

    fn get_provider_state(&self, provider_name: &str) -> Result<serde_json::Value> {
        if let Ok(ps) = self.provider_states.try_read() {
            Ok(ps.get(provider_name).cloned().unwrap_or(serde_json::Value::Null))
        } else {
            Ok(serde_json::Value::Null)
        }
    }

    fn set_provider_state(&self, provider_name: &str, state: serde_json::Value) -> Result<()> {
        if let Ok(mut ps) = self.provider_states.try_write() {
            ps.set(provider_name, state);
        }
        Ok(())
    }

    async fn add_messages_batch(&self, messages: &[ChatMessage]) -> Result<()> {
        let count = messages.len() as u64;
        {
            let mut history = self.history.write().await;
            history.extend(messages.iter().cloned());
        }
        let mut meta = self.metadata.write().await;
        meta.message_count += count;
        meta.updated_at = Utc::now();
        Ok(())
    }

    fn get_message_count(&self) -> usize {
        self.metadata.try_read().map(|m| m.message_count as usize).unwrap_or(0)
    }

    fn touch_request_hash(&self, messages: &[ChatMessage]) {
        let hash = hash_messages(messages);
        if let Ok(mut meta) = self.metadata.try_write() {
            meta.last_request_hash = Some(hash);
        }
    }

    fn get_last_request_hash(&self) -> Option<u64> {
        self.metadata.try_read().ok().and_then(|m| m.last_request_hash)
    }
}

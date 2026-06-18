use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{ChatMessage, Result, AgentError};

/// 会话 TTL 选项，用于控制会话的生命周期。
#[derive(Debug, Clone)]
pub struct SessionTTLOptions {
    /// 会话清理前的最大空闲时间（秒）
    pub max_idle_secs: Option<u64>,
    /// 会话强制清理前的最大存活时间（秒）
    pub max_lifetime_secs: Option<u64>,
    /// 清理检查的间隔时间（秒）
    pub cleanup_interval_secs: u64,
}

impl Default for SessionTTLOptions {
    fn default() -> Self {
        Self {
            max_idle_secs: None,
            max_lifetime_secs: None,
            cleanup_interval_secs: 3600, // 1 hour default
        }
    }
}

/// Session — 等同于 MAF 的 AgentSession，管理多轮消息的生命周期。
#[async_trait]
pub trait ISession: Send + Sync {
    /// 获取会话 ID
    fn session_id(&self) -> &str;
    /// 添加一条消息到会话历史
    async fn add_message(&self, message: ChatMessage) -> Result<()>;
    /// 获取会话中的所有消息
    async fn get_messages(&self) -> Result<Vec<ChatMessage>>;
    /// 清空会话历史
    async fn clear(&self) -> Result<()>;
    /// 获取会话元数据
    fn metadata(&self) -> SessionMetadata;
    /// 获取会话快照（用于调试/展示）
    fn snapshot(&self) -> SessionSnapshot;
    /// 序列化会话为 JSON 字符串
    fn serialize(&self) -> Result<String>;
    /// 从 JSON 字符串反序列化会话
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

    /// 获取会话创建时间戳
    fn created_at(&self) -> DateTime<Utc> {
        Utc::now() // 默认回退
    }

    /// 获取最后活动时间戳
    fn last_active_at(&self) -> DateTime<Utc> {
        Utc::now() // 默认回退
    }

    /// 更新最后活动时间戳
    async fn touch_last_active(&self) {}
}

/// 会话元数据 — 创建时间、消息数量、缓存追踪哈希
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u64,
    /// 上次请求完整消息数组的哈希，用于 KV 缓存前缀追踪
    pub last_request_hash: Option<u64>,
}

/// Provider 级 Session 状态存储
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderStateStore {
    states: HashMap<String, serde_json::Value>,
}

impl ProviderStateStore {
    /// 创建空的 Provider 状态存储
    pub fn new() -> Self { Self { states: HashMap::new() } }
    /// 获取指定 Provider 的状态
    pub fn get(&self, provider_name: &str) -> Option<&serde_json::Value> {
        self.states.get(provider_name)
    }
    /// 设置指定 Provider 的状态
    pub fn set(&mut self, provider_name: &str, state: serde_json::Value) {
        self.states.insert(provider_name.to_string(), state);
    }
    /// 移除指定 Provider 的状态
    pub fn remove(&mut self, provider_name: &str) { self.states.remove(provider_name); }
}

/// 类型安全的 Provider 状态访问器
///
/// 为 Provider 提供类型安全的 Session 状态读写能力，
/// 避免手动序列化/反序列化和 key 拼写错误。
///
/// ## 使用方式
///
/// ```ignore
/// struct MyState { last_count: usize }
///
/// let state_key = ProviderState::<MyState>::new("MyProvider");
/// let state = state_key.get_or_init(&session);
/// state.last_count += 1;
/// state_key.save(&session, &state)?;
/// ```
pub struct ProviderState<T: serde::Serialize + serde::de::DeserializeOwned> {
    key: String,
    _marker: std::marker::PhantomData<T>,
}

impl<T: serde::Serialize + serde::de::DeserializeOwned + Default> ProviderState<T> {
    /// 创建指定 Provider 的类型安全状态访问器
    pub fn new(provider_name: &str) -> Self {
        Self {
            key: format!("provider_state::{}", provider_name),
            _marker: std::marker::PhantomData,
        }
    }

    /// 获取或初始化 Provider 状态
    ///
    /// 如果 Session 中存在该 Provider 的状态，反序列化返回；
    /// 否则返回 T 的默认值。
    pub fn get_or_init(&self, session: &dyn ISession) -> T {
        session
            .get_provider_state(&self.key)
            .ok()
            .and_then(|v| serde_json::from_value::<T>(v).ok())
            .unwrap_or_default()
    }

    /// 保存 Provider 状态到 Session
    pub fn save(&self, session: &dyn ISession, state: &T) -> Result<()> {
        let value =
            serde_json::to_value(state).map_err(|e| AgentError::Serialize(e.to_string()))?;
        session.set_provider_state(&self.key, value)
    }
}

/// 只读会话快照，用于调试、UI 展示、审计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub metadata: SessionMetadata,
    pub messages: Vec<ChatMessage>,
    pub provider_states: ProviderStateStore,
    /// 最后活动时间戳 — 跨序列化保留，用于 TTL 追踪
    pub last_active_at: Option<DateTime<Utc>>,
}

/// 默认的内存会话实现
pub struct AgentSession {
    session_id: String,
    history: RwLock<Vec<ChatMessage>>,
    metadata: RwLock<SessionMetadata>,
    provider_states: RwLock<ProviderStateStore>,
    created_at: DateTime<Utc>,
    last_active_at: RwLock<DateTime<Utc>>,
}

impl AgentSession {
    /// 创建新的内存会话，自动生成随机 UUID 作为会话 ID
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            session_id: Uuid::new_v4().to_string(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: now, updated_at: now, message_count: 0, last_request_hash: None,
            }),
            provider_states: RwLock::new(ProviderStateStore::new()),
            created_at: now,
            last_active_at: RwLock::new(now),
        }
    }

    /// 使用指定 ID 创建新的内存会话
    pub fn with_id(session_id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            session_id: session_id.into(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: now, updated_at: now, message_count: 0, last_request_hash: None,
            }),
            provider_states: RwLock::new(ProviderStateStore::new()),
            created_at: now,
            last_active_at: RwLock::new(now),
        }
    }

    /// 记录本次请求的消息哈希（用于 KV 缓存前缀追踪）
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

/// 计算消息列表的哈希值（用于 KV 缓存前缀追踪）
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
                call.arguments
                    .as_str()
                    .unwrap_or("")
                    .hash(&mut hasher);
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

    /// 追加一条消息到会话历史
    async fn add_message(&self, message: ChatMessage) -> Result<()> {
        self.history.write().await.push(message);
        let mut meta = self.metadata.write().await;
        meta.message_count += 1;
        meta.updated_at = Utc::now();
        Ok(())
    }

    /// 获取会话中的所有消息副本
    async fn get_messages(&self) -> Result<Vec<ChatMessage>> {
        Ok(self.history.read().await.clone())
    }

    /// 清空会话历史并重置消息计数
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

    /// 返回当前会话状态的最佳快照（尽力而为）
    ///
    /// 注意：每个字段通过独立的 `try_read()` 调用读取——快照不是原子一致的。
    /// 消息、元数据和 Provider 状态可能反映略有不同的时间点。
    /// 仅用于调试/展示，不保证跨字段一致性。
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
            last_active_at: self.last_active_at.try_read().ok().map(|t| *t),
        }
    }

    /// 序列化会话快照为 JSON 字符串
    fn serialize(&self) -> Result<String> {
        serde_json::to_string(&self.snapshot())
            .map_err(|e| AgentError::Serialize(e.to_string()))
    }

    /// 从 JSON 字符串反序列化恢复会话
    fn deserialize(data: &str) -> Result<Self> {
        let snap: SessionSnapshot = serde_json::from_str(data)
            .map_err(|e| AgentError::Serialize(e.to_string()))?;
        let created_at = snap.metadata.created_at;
        let last_active = snap.last_active_at.unwrap_or(created_at);
        Ok(Self {
            session_id: snap.session_id,
            history: RwLock::new(snap.messages),
            metadata: RwLock::new(snap.metadata),
            provider_states: RwLock::new(snap.provider_states),
            created_at,
            last_active_at: RwLock::new(last_active),
        })
    }

    /// 获取指定 Provider 在会话中存储的状态
    fn get_provider_state(&self, provider_name: &str) -> Result<serde_json::Value> {
        if let Ok(ps) = self.provider_states.try_read() {
            Ok(ps.get(provider_name).cloned().unwrap_or(serde_json::Value::Null))
        } else {
            Ok(serde_json::Value::Null)
        }
    }

    /// 设置指定 Provider 在会话中的状态
    fn set_provider_state(&self, provider_name: &str, state: serde_json::Value) -> Result<()> {
        if let Ok(mut ps) = self.provider_states.try_write() {
            ps.set(provider_name, state);
        }
        Ok(())
    }

    /// 原子批量追加多条消息到会话历史
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

    /// 获取当前消息数量（O(1)，无克隆）
    fn get_message_count(&self) -> usize {
        self.metadata.try_read().map(|m| m.message_count as usize).unwrap_or(0)
    }

    /// 记录本次请求的消息哈希（KV 缓存前缀追踪）
    fn touch_request_hash(&self, messages: &[ChatMessage]) {
        let hash = hash_messages(messages);
        if let Ok(mut meta) = self.metadata.try_write() {
            meta.last_request_hash = Some(hash);
        }
    }

    /// 获取上次请求的消息哈希
    fn get_last_request_hash(&self) -> Option<u64> {
        self.metadata.try_read().ok().and_then(|m| m.last_request_hash)
    }

    /// 获取会话创建时间戳
    fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// 获取最后活动时间戳
    fn last_active_at(&self) -> DateTime<Utc> {
        self.last_active_at.try_read().map(|t| *t).unwrap_or(self.created_at)
    }

    /// 更新最后活动时间戳为当前时间
    async fn touch_last_active(&self) {
        let mut t = self.last_active_at.write().await;
        *t = Utc::now();
    }
}

# 6.2 AgentSession 默认实现

`AgentSession` 是 `ISession` 的默认内存实现。它使用 `RwLock` 保护并发访问，自动生成 UUID 作为会话 ID，并提供完整的消息管理、元数据追踪和序列化支持。

## 结构定义

```rust
/// 默认的内存会话实现
pub struct AgentSession {
    session_id: String,                          // UUID v4
    history: RwLock<Vec<ChatMessage>>,           // 消息历史
    metadata: RwLock<SessionMetadata>,           // 会话元数据
    provider_states: RwLock<ProviderStateStore>, // Provider 状态存储
    created_at: DateTime<Utc>,                   // 创建时间（不可变）
    last_active_at: RwLock<DateTime<Utc>>,       // 最后活动时间
}
```

**并发策略**：

- 每个字段独立使用 `RwLock` 保护，降低锁竞争
- `RwLock` 而非 `Mutex`：消息读取远多于写入，读写锁更高效
- `created_at` 不可变，无需锁

## 构造函数

```rust
impl AgentSession {
    /// 创建新的内存会话，自动生成随机 UUID 作为会话 ID
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
            // ... 同 new()
        }
    }
}

impl Default for AgentSession {
    fn default() -> Self { Self::new() }
}
```

**`new()` vs `with_id()`**：`new()` 适用于全新会话（自动生成 UUID）；`with_id()` 适用于恢复已有会话（如从文件系统反序列化后）。

## ISession 实现详解

### 消息操作

```rust
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
        Ok(self.history.read().await.clone())  // 返回完整副本
    }

    async fn clear(&self) -> Result<()> {
        self.history.write().await.clear();
        let mut meta = self.metadata.write().await;
        meta.message_count = 0;
        meta.updated_at = Utc::now();
        Ok(())
    }
}
```

**设计考量**：

- `get_messages()` 返回完整副本——牺牲内存效率换取接口简洁性，避免引用生命周期复杂化
- `add_message()` 和 `clear()` 同时更新元数据——保证 `message_count` 和 `updated_at` 的一致性

### 批量操作

```rust
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
    self.metadata.try_read()
        .map(|m| m.message_count as usize)
        .unwrap_or(0)
}
```

**优化点**：

- `add_messages_batch` 覆盖默认实现，一次性持有写锁完成所有追加（而非逐条加锁）
- `get_message_count` 实现为 O(1) 零克隆——读 `metadata` 中的 `message_count` 而非遍历 `history`

### 元数据与快照

```rust
fn metadata(&self) -> SessionMetadata {
    self.metadata.try_read()
        .map(|m| m.clone())
        .unwrap_or_default()
}

fn snapshot(&self) -> SessionSnapshot {
    // 注意：非原子一致性——各字段通过独立 try_read() 获取
    let history = self.history.try_read()
        .map(|h| h.clone()).unwrap_or_default();
    let meta = self.metadata.try_read()
        .map(|m| m.clone()).unwrap_or_default();
    let ps = self.provider_states.try_read()
        .map(|p| p.clone()).unwrap_or_default();

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
```

**快照的非原子性**：由于每个字段通过独立的 `try_read()` 读取，快照不是原子一致的。消息、元数据和 Provider 状态可能反映略有不同的时间点。框架在设计上接受这种最终一致性。

### 序列化

```rust
fn serialize(&self) -> Result<String> {
    serde_json::to_string(&self.snapshot())
        .map_err(|e| AgentError::Serialize(e.to_string()))
}

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
```

**序列化路径**：通过 `snapshot()` → `SessionSnapshot` → `serde_json`，所有字段都是 JSON 可序列化的。

### 请求哈希追踪（KV 缓存优化）

```rust
fn touch_request_hash(&self, messages: &[ChatMessage]) {
    let hash = hash_messages(messages);
    if let Ok(mut meta) = self.metadata.try_write() {
        meta.last_request_hash = Some(hash);
    }
}

fn get_last_request_hash(&self) -> Option<u64> {
    self.metadata.try_read().ok().and_then(|m| m.last_request_hash)
}
```

**哈希算法**：

```rust
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
                call.arguments.as_str().unwrap_or("").hash(&mut hasher);
            }
        }
        if let Some(ref tcid) = msg.tool_call_id {
            tcid.hash(&mut hasher);
        }
    }
    hasher.finish()
}
```

哈希基于消息的关键字段：role、content、tool_calls（id + name + arguments）、tool_call_id。新请求可以比较消息前缀与前次请求的哈希，如果前缀匹配则可能复用 KV 缓存。

### TTL 支持

```rust
fn created_at(&self) -> DateTime<Utc> {
    self.created_at
}

fn last_active_at(&self) -> DateTime<Utc> {
    self.last_active_at.try_read().map(|t| *t).unwrap_or(self.created_at)
}

async fn touch_last_active(&self) {
    let mut t = self.last_active_at.write().await;
    *t = Utc::now();
}
```

### Provider 状态

```rust
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
```

## ProviderStateStore

```rust
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
    pub fn remove(&mut self, provider_name: &str) {
        self.states.remove(provider_name);
    }
}
```

这是一个按 Provider 名称索引的通用 JSON 状态存储。每个 Provider 可以在此存储任意 JSON 值，框架不关心内容的具体结构。

## 关键要点

1. **每个字段独立 RwLock**——降低锁竞争，提升并发性能
2. **`get_messages()` 返回完整副本**——简单可靠，适合典型对话规模（数百条消息）
3. **快照非原子一致**——接受最终一致性，避免全局锁
4. **批量操作一次持锁**——`add_messages_batch` 一次性完成所有追加
5. **request_hash 支持 KV 缓存优化**——对现代 LLM 提供商的性能至关重要

# 6.1 ISession 会话接口

`ISession` 是 RAF 会话管理的核心 trait。它定义了多轮对话的状态管理契约——消息历史的增删查、序列化/反序列化、Provider 状态存取、KV 缓存前缀追踪和会话 TTL 支持。

## ISession trait 完整定义

```rust
/// Session — 等同于 MAF 的 AgentSession，管理多轮消息的生命周期。
#[async_trait]
pub trait ISession: Send + Sync {
    // ── 身份 ──
    fn session_id(&self) -> &str;

    // ── 消息管理（核心） ──
    async fn add_message(&self, message: ChatMessage) -> Result<()>;
    async fn get_messages(&self) -> Result<Vec<ChatMessage>>;
    async fn clear(&self) -> Result<()>;

    // ── 批量操作 ──
    async fn add_messages_batch(&self, messages: &[ChatMessage]) -> Result<()> {
        for msg in messages {
            self.add_message(msg.clone()).await?;
        }
        Ok(())
    }
    fn get_message_count(&self) -> usize { 0 }

    // ── 元数据 ──
    fn metadata(&self) -> SessionMetadata;
    fn snapshot(&self) -> SessionSnapshot;

    // ── 序列化 ──
    fn serialize(&self) -> Result<String>;
    fn deserialize(data: &str) -> Result<Self> where Self: Sized;

    // ── Provider 状态 ──
    fn get_provider_state(&self, _provider_name: &str) -> Result<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }
    fn set_provider_state(&self, _provider_name: &str, _state: serde_json::Value) -> Result<()> {
        Ok(())
    }

    // ── KV 缓存前缀追踪 ──
    fn touch_request_hash(&self, _messages: &[ChatMessage]) {}
    fn get_last_request_hash(&self) -> Option<u64> { None }

    // ── TTL 支持 ──
    fn created_at(&self) -> DateTime<Utc> { DateTime::UNIX_EPOCH }
    fn last_active_at(&self) -> DateTime<Utc> { DateTime::UNIX_EPOCH }
    async fn touch_last_active(&self) {}
}
```

### 方法分类详解

#### 身份识别

| 方法 | 返回 | 说明 |
|------|------|------|
| `session_id()` | `&str` | 返回会话唯一标识符，通常是 UUID v4 |

#### 消息管理（核心 API）

| 方法 | 签名 | 说明 |
|------|------|------|
| `add_message()` | `async fn add_message(&self, message: ChatMessage) -> Result<()>` | 追加一条消息到会话历史 |
| `get_messages()` | `async fn get_messages(&self) -> Result<Vec<ChatMessage>>` | 获取会话中所有消息的副本 |
| `clear()` | `async fn clear(&self) -> Result<()>` | 清空会话历史 |
| `add_messages_batch()` | `async fn add_messages_batch(&self, messages: &[ChatMessage]) -> Result<()>` | 批量追加消息，默认实现逐条调用 `add_message()`；可重写实现原子写入 |
| `get_message_count()` | `fn get_message_count(&self) -> usize` | 获取消息数量（O(1)，零克隆），默认返回 0 |

**设计意图**：`add_messages_batch` 和 `get_message_count` 有默认实现，使最小化实现只需关注 3 个核心方法（`add_message`、`get_messages`、`clear`）。

#### 元数据与序列化

| 方法 | 返回 | 说明 |
|------|------|------|
| `metadata()` | `SessionMetadata` | 返回会话元数据（创建时间、更新时间、消息计数、请求哈希） |
| `snapshot()` | `SessionSnapshot` | 返回会话的完整快照（消息 + 元数据 + Provider 状态，用于调试/展示） |
| `serialize()` | `Result<String>` | 序列化会话为 JSON 字符串 |
| `deserialize()` | `Result<Self>` | 从 JSON 字符串反序列化恢复会话 |

#### Provider 状态

| 方法 | 说明 |
|------|------|
| `get_provider_state(name)` | 获取指定 Provider 的持久化状态（JSON Value） |
| `set_provider_state(name, state)` | 设置指定 Provider 的持久化状态 |

这两个方法使 Provider 可以在多次 `run()` 调用间存储状态（如进度计数器、缓存数据）。

#### KV 缓存前缀追踪

| 方法 | 说明 |
|------|------|
| `touch_request_hash(messages)` | 记录本次请求的 messages 哈希值 |
| `get_last_request_hash()` | 获取上次请求的 messages 哈希值 |

**用途**：部分 LLM 提供商（如 DeepSeek）支持基于消息前缀的 KV 缓存。通过追踪请求哈希，框架可以判断新请求的消息列表是否共享之前请求的前缀，从而利用缓存的 KV 值加速推理。

#### TTL 支持

| 方法 | 说明 |
|------|------|
| `created_at()` | 会话创建时间戳。默认返回 `DateTime::UNIX_EPOCH`（哨兵值），实现者**必须覆写**以返回实际时间，否则 TTL 清理将无法正确判断会话是否过期 |
| `last_active_at()` | 最后活动时间戳。默认返回 `DateTime::UNIX_EPOCH`（哨兵值），实现者**必须覆写**以返回实际时间 |
| `touch_last_active()` | 更新最后活动时间戳为当前时间 |

> **设计说明**：默认实现返回 `DateTime::UNIX_EPOCH` 而非 `Utc::now()`。`Utc::now()` 作为默认值会导致未覆写 `created_at` / `last_active_at` 的实现者每次调用都返回不同的时间戳，造成 TTL 计算错误（新会话被误判为刚创建，过期会话被误判为活跃）。哨兵值使问题显式化——如果 TTL 清理发现所有会话的创建时间都是 1970-01-01，说明具体实现未正确覆写这两个方法。

## SessionMetadata

```rust
/// 会话元数据 — 创建时间、消息数量、缓存追踪哈希
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u64,
    /// 上次请求完整消息数组的哈希，用于 KV 缓存前缀追踪
    pub last_request_hash: Option<u64>,
}
```

## SessionSnapshot

快照是会话当前状态的只读副本，用于调试、UI 展示和审计：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub metadata: SessionMetadata,
    pub messages: Vec<ChatMessage>,
    pub provider_states: ProviderStateStore,
    /// 最后活动时间戳 — 跨序列化保留，用于 TTL 追踪
    pub last_active_at: Option<DateTime<Utc>>,
}
```

**注意**：快照不是原子一致的——消息、元数据和 Provider 状态通过独立的 `try_read()` 调用获取，可能反映略有不同的时间点。仅用于调试/展示，不保证跨字段一致性。

## SessionTTLOptions

```rust
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
```

| 字段 | 含义 | 典型值 |
|------|------|--------|
| `max_idle_secs` | 会话空闲超过此秒数后被清理 | 3600（1 小时） |
| `max_lifetime_secs` | 会话从创建起超过此秒数后强制清理 | 86400（24 小时） |
| `cleanup_interval_secs` | 清理任务执行的间隔 | 3600（1 小时） |

## 会话生命周期

```mermaid
stateDiagram-v2
    [*] --> Active: AgentSession::new()
    Active --> Active: add_message()<br/>touch_last_active()
    Active --> Idle: 无活动
    Idle --> Active: run() 被调用
    Idle --> Expired: now - last_active > max_idle_secs
    Active --> Expired: now - created_at > max_lifetime_secs
    Expired --> [*]: cleanup_expired() 删除

    note right of Expired
        idle 和 lifetime 任一超时
        即标记为过期
    end note
```

## 关键要点

1. **最小实现只需 3 个方法**——`add_message()`、`get_messages()`、`clear()`，其余均有默认实现
2. **批量操作默认逐条执行**——`add_messages_batch` 可通过重写实现原子写入
3. **request_hash 用于 KV 缓存优化**——让 LLM 提供商利用消息前缀缓存加速推理
4. **TTL 由存储层实现**——`ISession` 只提供时间戳，`ISessionStore::cleanup_expired()` 执行实际清理
5. **序列化通过快照实现**——`serialize()` 内部调用 `snapshot()`，然后 JSON 序列化

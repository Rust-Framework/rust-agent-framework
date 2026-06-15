# RAF 改进实施计划：Session 管理与上下文工程体系重构

## 一、概述

基于 RAF 与微软 MAF 框架的对比分析，识别出核心缺陷。本计划按优先级分阶段实施改进。

**核心决策（用户已确认）**：
1. ToolLoopAgent 按 MAF 管道模式重构，工具循环下沉到 IChatClient 层
2. Session 注册中心参照 MAF 的 AIHostAgent 模式，由 AgentHost 统一管理，保持 `run()` 传入 Session
3. 会话隔离采用装饰器模式
4. 命名：`ModelInfo` → `ModelMetadata`，`SessionTTLConfig` → `SessionTTLOptions`

**已完成的工作**（无需重复实施）：
- P0 全部完成：ModelMetadata、ITokenCounter + EstimateCounter/TiktokenCounter、ICompressionStrategy + SlidingWindow/TokenBudget/Pipeline、ChatClientAgent Phase 1.5 压缩集成
- P1 部分完成：ISessionStore + InMemory/FileSystem 实现、SessionTTLOptions + ISession TTL 字段、IsolationScopedSessionStore 装饰器、MessageSource 枚举 + ChatMessage.source 字段

---

## 二、待实施任务

### 任务 1：AgentHost — Session 注册中心/生命周期管理

**参照**：MAF 的 `AIHostAgent`（继承 `DelegatingAIAgent`，持有 `AgentSessionStore`）

**新建文件**：`crates/framework/src/agent_host.rs`

```rust
use async_trait::async_trait;
use std::sync::Arc;
use rust_agent_core::{
    AgentRunOptions, BoxStream, ChatMessage, IAgent, ISession, ISessionStore,
    Result, SessionTTLOptions, AgentResponseResult,
};

/// Agent 托管主机，提供 Session 注册中心和生命周期管理
/// 参照 MAF 的 AIHostAgent 设计
pub struct AgentHost {
    agent: Arc<dyn IAgent>,
    session_store: Arc<dyn ISessionStore>,
    ttl_options: Option<SessionTTLOptions>,
}

impl AgentHost {
    pub fn new(agent: Arc<dyn IAgent>, session_store: Arc<dyn ISessionStore>) -> Self {
        Self { agent, session_store, ttl_options: None }
    }

    pub fn with_ttl_options(mut self, options: SessionTTLOptions) -> Self {
        self.ttl_options = Some(options);
        self
    }

    /// 获取或创建 Session
    /// 如果 session_id 对应的 Session 存在于 Store 中，加载并返回
    /// 否则通过 agent 创建新 Session 并存入 Store
    pub async fn get_or_create_session(&self, session_id: &str) -> Result<Arc<dyn ISession>> {
        if let Some(session) = self.session_store.get_session(session_id).await? {
            session.touch_last_active().await;
            return Ok(session);
        }
        // 创建新 Session（当前使用 AgentSession::with_id）
        let session: Arc<dyn ISession> = Arc::new(
            rust_agent_core::AgentSession::with_id(session_id)
        );
        self.session_store.save_session(session.as_ref()).await?;
        Ok(session)
    }

    /// 保存 Session 到 Store
    pub async fn save_session(&self, session: &Arc<dyn ISession>) -> Result<()> {
        self.session_store.save_session(session.as_ref()).await
    }

    /// 运行 Agent，自动管理 Session 生命周期
    /// 保持 run() 传入 Arc<dyn ISession> 的签名（用户明确要求）
    pub async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Arc<dyn ISession>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        // 1. touch + save before run
        session.touch_last_active().await;
        let _ = self.session_store.save_session(session.as_ref()).await;

        // 2. run agent
        let stream = self.agent.run(messages, Some(session.clone()), options).await?;

        // 3. spawn: stream 完成后 save session
        let store = self.session_store.clone();
        tokio::spawn(async move {
            let _ = store.save_session(session.as_ref()).await;
        });

        Ok(stream)
    }

    /// 清理过期 Session
    pub async fn cleanup_expired(&self) -> Result<usize> {
        self.session_store.cleanup_expired().await
    }
}
```

**修改文件**：`crates/framework/src/lib.rs`
- 新增 `pub mod agent_host;`
- 新增 `pub use agent_host::AgentHost;`

**验证**：`cargo check`

---

### 任务 2：IChatClient 管道重构 — ChatClientBuilder + FunctionInvokingChatClient

**参照**：MAF 的 `ChatClientBuilder` + `FunctionInvokingChatClient` + `DelegatingChatClient` 管道模式

这是最核心的架构变更。当前 RAF 的工具循环在 Agent 层（ToolLoopAgent 包装 ChatClientAgent），MAF 的做法是在 ChatClient 层通过装饰器管道实现。

#### 2.1 新增 DelegatingChatClient 基类

**修改文件**：`crates/core/src/chat_client.rs`

在 `IChatClient` trait 下方新增：

```rust
/// ChatClient 装饰器基类，参照 MAF 的 DelegatingChatClient
/// 所有未重写的方法透传给 inner client
pub struct DelegatingChatClient {
    inner: Arc<dyn IChatClient>,
}

impl DelegatingChatClient {
    pub fn new(inner: Arc<dyn IChatClient>) -> Self {
        Self { inner }
    }

    pub fn inner(&self) -> &Arc<dyn IChatClient> {
        &self.inner
    }
}

#[async_trait]
impl IChatClient for DelegatingChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        self.inner.run(messages, options).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.inner.model_metadata()
    }
}
```

**修改文件**：`crates/core/src/lib.rs`
- 新增 re-export `DelegatingChatClient`

#### 2.2 新增 ChatClientBuilder

**修改文件**：`crates/core/src/chat_client.rs`

```rust
/// ChatClient 管道构建器，参照 MAF 的 ChatClientBuilder
/// 按注册顺序逆序包装装饰器，最终形成管道链
pub struct ChatClientBuilder {
    decorators: Vec<Box<dyn Fn(Arc<dyn IChatClient>) -> Arc<dyn IChatClient> + Send + Sync>>,
    leaf: Option<Arc<dyn IChatClient>>,
}

impl ChatClientBuilder {
    pub fn new() -> Self {
        Self { decorators: Vec::new(), leaf: None }
    }

    /// 设置叶子 ChatClient（实际的 LLM 服务客户端）
    pub fn leaf(mut self, client: Arc<dyn IChatClient>) -> Self {
        self.leaf = Some(client);
        self
    }

    /// 添加装饰器工厂
    pub fn use_decorator(
        mut self,
        factory: Box<dyn Fn(Arc<dyn IChatClient>) -> Arc<dyn IChatClient> + Send + Sync>,
    ) -> Self {
        self.decorators.push(factory);
        self
    }

    /// 构建管道：decorators[0] 包装 leaf，decorators[1] 包装上一层，以此类推
    pub fn build(self) -> Result<Arc<dyn IChatClient>> {
        let mut client = self.leaf.ok_or_else(|| {
            rust_agent_core::AgentError::ConfigError("leaf IChatClient is required".into())
        })?;
        for factory in self.decorators {
            client = factory(client);
        }
        Ok(client)
    }
}

impl Default for ChatClientBuilder {
    fn default() -> Self { Self::new() }
}
```

**修改文件**：`crates/core/src/lib.rs`
- 新增 re-export `ChatClientBuilder`

#### 2.3 新增 FunctionInvokingChatClient

**新建文件**：`crates/framework/src/chat_client_decorators/mod.rs`

```rust
pub mod function_invoking;
pub mod per_service_call_persisting;

pub use function_invoking::FunctionInvokingChatClient;
pub use per_service_call_persisting::PerServiceCallPersistingChatClient;
```

**新建文件**：`crates/framework/src/chat_client_decorators/function_invoking.rs`

核心逻辑：将 ToolLoopAgent 的工具循环逻辑迁移到 IChatClient 装饰器中。

```rust
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage,
    Content, DelegatingChatClient, FinishReason, IChatClient, ITool,
    ModelMetadata, Result, ToolCalledContent, ToolCallingContent, ToolCall,
};
use tokio::sync::mpsc;

/// 工具调用循环 ChatClient 装饰器
/// 参照 MAF 的 FunctionInvokingChatClient 设计
/// 将工具调用循环从 Agent 层下沉到 ChatClient 管道层
pub struct FunctionInvokingChatClient {
    inner: Arc<dyn IChatClient>,
    tools: Vec<Arc<dyn ITool>>,
    max_rounds: usize,
}

impl FunctionInvokingChatClient {
    pub fn new(inner: Arc<dyn IChatClient>, tools: Vec<Arc<dyn ITool>>) -> Self {
        Self { inner, tools, max_rounds: 10 }
    }

    pub fn with_max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds;
        self
    }

    pub fn tools(&self) -> &[Arc<dyn ITool>] {
        &self.tools
    }
}

#[async_trait]
impl IChatClient for FunctionInvokingChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        // 实现工具循环：
        // 1. 调用 inner.run(messages, options)
        // 2. 消费流，收集 ToolCallingContent
        // 3. 如果有工具调用：并行执行工具，将 assistant(tool_calls) + tool(results) 追加到 messages，回到步骤 1
        // 4. 如果无工具调用，返回最终流
        // 使用 unfold + mpsc channel 模式（与当前 ToolLoopAgent 相同的状态机模式）
        todo!("迁移 ToolLoopAgent 的循环逻辑到此处")
    }

    fn model_id(&self) -> &str { self.inner.model_id() }
    fn model_metadata(&self) -> Option<&ModelMetadata> { self.inner.model_metadata() }
}
```

**关键设计决策**：
- `FunctionInvokingChatClient` 是 `IChatClient` 的装饰器（不是 IAgent）
- 工具循环在 ChatClient 层完成，`ChatClientAgent` 只调用一次 `chat_client.run()`
- 每轮迭代自行维护累积消息列表，不依赖 `InMemoryHistoryProvider`
- 消息写入统一通过 Provider 管道，不再直接写 Session

#### 2.4 新增 PerServiceCallPersistingChatClient

**新建文件**：`crates/framework/src/chat_client_decorators/per_service_call_persisting.rs`

```rust
use async_trait::async_trait;
use std::sync::Arc;
use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage,
    DelegatingChatClient, IChatClient, ModelMetadata, Result,
};

/// 每轮服务调用后持久化的 ChatClient 装饰器
/// 参照 MAF 的 PerServiceCallChatHistoryPersistingChatClient 设计
/// 在 FunctionInvokingChatClient 和 Leaf Client 之间插入
/// 每轮 LLM 调用后触发 Provider 的 on_invoked() 持久化
/// 确保工具循环中途失败时不丢失中间状态
pub struct PerServiceCallPersistingChatClient {
    inner: Arc<dyn IChatClient>,
    // 需要某种方式访问 Provider 管道和 Session
    // 方案：通过闭包回调或 trait object 注入持久化逻辑
    persist_callback: Box<dyn Fn(Vec<ChatMessage>) + Send + Sync>,
}

impl PerServiceCallPersistingChatClient {
    pub fn new(
        inner: Arc<dyn IChatClient>,
        persist_callback: Box<dyn Fn(Vec<ChatMessage>) + Send + Sync>,
    ) -> Self {
        Self { inner, persist_callback }
    }
}

#[async_trait]
impl IChatClient for PerServiceCallPersistingChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let stream = self.inner.run(messages, options).await?;
        // 消费流后调用 persist_callback
        todo!("实现流消费后的回调触发")
    }

    fn model_id(&self) -> &str { self.inner.model_id() }
    fn model_metadata(&self) -> Option<&ModelMetadata> { self.inner.model_metadata() }
}
```

#### 2.5 重构 ChatClientAgent 使用管道

**修改文件**：`crates/framework/src/chat_client_agent.rs`

变更要点：
- 构造时通过 `ChatClientBuilder` 构建管道：`FunctionInvokingChatClient → [PerServiceCallPersistingChatClient] → Leaf IChatClient`
- `run()` 方法只调用一次 `chat_client.run()`，工具循环由管道自动完成
- 移除对 `ToolLoopAgent` 的依赖
- 消息写入统一通过 Provider 管道

**修改文件**：`crates/framework/src/builder.rs`

变更要点：
- `build()` 方法使用 `ChatClientBuilder` 构建管道
- 当有 tools 时，不再创建 `ToolLoopAgent` 包装，而是将 tools 传入 `FunctionInvokingChatClient`
- 管道构建顺序：`FunctionInvokingChatClient(tools) → Leaf IChatClient`

#### 2.6 废弃 ToolLoopAgent

**修改文件**：`crates/framework/src/agents/tool_loop_agent.rs`

- 在结构体和 impl 块上添加 `#[deprecated(since = "0.2.0", note = "Use ChatClientBuilder + FunctionInvokingChatClient instead")]`
- 保留文件以兼容，但推荐使用管道模式

**修改文件**：`crates/framework/src/lib.rs`
- 新增 `pub mod chat_client_decorators;`
- 新增 re-export `FunctionInvokingChatClient`, `PerServiceCallPersistingChatClient`, `ChatClientBuilder`

**验证**：`cargo check` + `cargo test`

---

### 任务 3：消除静默错误吞没

**修改文件**：

1. `crates/framework/src/chat_client_agent.rs` 第 330 行：
   ```rust
   // 之前：
   let _ = sess.add_message(ChatMessage::assistant(response.text.clone())).await;
   // 之后：
   if let Err(e) = sess.add_message(ChatMessage::assistant(response.text.clone())).await {
       tracing::warn!(error = %e, "Failed to persist assistant message to session");
   }
   ```

2. `crates/framework/src/agents/tool_loop_agent.rs` 第 291-319 行（所有 `let _ = sess.add_message(...)` 和 `let _ = sess.add_message(...)` ）：
   ```rust
   // 之前：
   let _ = sess.add_message(...).await;
   // 之后：
   if let Err(e) = sess.add_message(...).await {
       tracing::warn!(error = %e, call_id = %tc.call_id, "Failed to persist tool interaction to session");
   }
   ```

3. `crates/framework/src/context_providers/history_provider.rs` 第 91 行：
   ```rust
   // 之前：
   let _ = session.add_messages_batch(&new_messages).await;
   // 之后：
   if let Err(e) = session.add_messages_batch(&new_messages).await {
       tracing::warn!(error = %e, count = new_messages.len(), "Failed to persist messages to session");
   }
   ```

**验证**：`cargo check`

---

### 任务 4：添加结构化日志

**修改文件**：

1. `crates/framework/src/chat_client_decorators/function_invoking.rs`（新建文件中）：
   - 工具循环每轮开始：`tracing::info!(round, tool_count, "Tool loop iteration")`
   - 工具执行：`tracing::debug!(tool_name, call_id, "Executing tool")`
   - 工具结果：`tracing::debug!(tool_name, call_id, has_error, "Tool execution completed")`
   - 达到 max_rounds：`tracing::warn!(max_rounds, "Tool loop reached max rounds")`

2. `crates/framework/src/agent_host.rs`（新建文件中）：
   - Session 创建：`tracing::info!(session_id, "Session created")`
   - Session 加载：`tracing::debug!(session_id, "Session loaded from store")`
   - Session 保存失败：`tracing::warn!(error = %e, session_id, "Session save failed")`
   - 清理过期：`tracing::info!(count, "Expired sessions cleaned up")`

**验证**：`cargo check`

---

### 任务 5：Provider 状态类型安全 ProviderState\<T\>

**修改文件**：`crates/core/src/session.rs`

在 `ProviderStateStore` 下方新增：

```rust
/// 类型安全的 Provider 状态访问器
pub struct ProviderState<T: serde::Serialize + serde::de::DeserializeOwned> {
    key: String,
    _marker: std::marker::PhantomData<T>,
}

impl<T: serde::Serialize + serde::de::DeserializeOwned + Default> ProviderState<T> {
    pub fn new(provider_name: &str) -> Self {
        Self {
            key: format!("provider_state::{}", provider_name),
            _marker: std::marker::PhantomData,
        }
    }

    pub fn get_or_init(&self, session: &dyn ISession) -> T {
        session.get_provider_state(&self.key)
            .ok()
            .and_then(|v| serde_json::from_value::<T>(v).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, session: &dyn ISession, state: &T) -> Result<()> {
        let value = serde_json::to_value(state)
            .map_err(|e| AgentError::Serialize(e.to_string()))?;
        session.set_provider_state(&self.key, value)
    }
}
```

**修改文件**：`crates/core/src/lib.rs`
- 新增 re-export `ProviderState`

**验证**：`cargo check`

---

### 任务 6：记忆/向量检索系统

**新建文件**：`crates/core/src/vector_store.rs`

```rust
use async_trait::async_trait;
use std::collections::HashMap;
use serde_json::Value;
use crate::Result;

/// 向量存储抽象
#[async_trait]
pub trait IVectorStore: Send + Sync {
    async fn upsert(&self, id: &str, embedding: Vec<f32>, metadata: HashMap<String, Value>) -> Result<()>;
    async fn search(&self, query_embedding: Vec<f32>, top_k: usize, filter: Option<HashMap<String, Value>>) -> Result<Vec<SearchResult>>;
    async fn delete(&self, id: &str) -> Result<()>;
}

pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub metadata: HashMap<String, Value>,
}
```

**新建文件**：`crates/framework/src/context_providers/memory_provider.rs`

```rust
/// 记忆上下文提供者，将聊天消息存入向量数据库
/// 参照 MAF 的 ChatHistoryMemoryProvider
pub struct MemoryContextProvider {
    vector_store: Arc<dyn IVectorStore>,
    mode: MemoryMode,
}

pub enum MemoryMode {
    AutoInject { top_k: usize },
    OnDemand,
}
```

**修改文件**：`crates/core/src/lib.rs` — 新增 `mod vector_store;` + re-export
**修改文件**：`crates/framework/src/lib.rs` — 新增 re-export

**验证**：`cargo check`

---

### 任务 7：ISession deserialize 多态支持

**修改文件**：`crates/core/src/agent.rs`

在 `IAgent` trait 中新增：

```rust
/// 创建此 Agent 专属类型的 Session
fn create_session(&self) -> Arc<dyn ISession>;

/// 从序列化数据恢复此 Agent 专属类型的 Session
fn deserialize_session(&self, data: &str) -> Result<Arc<dyn ISession>>;
```

默认实现返回 `AgentSession`，具体 Agent 可覆盖以创建自定义 Session 类型。

**修改文件**：所有实现 `IAgent` 的类型需添加这两个方法的实现
- `crates/framework/src/chat_client_agent.rs` — ChatClientAgent + AgentProxy
- `crates/framework/src/agents/tool_loop_agent.rs` — ToolLoopAgent

**验证**：`cargo check`

---

### 任务 8：工作流 Session 传递

**修改文件**：`crates/workflow/src/` 下所有 Pattern 实现

- 所有 `agent.run()` 调用改为传入 `session.clone()` 而非 `None`
- 确保 Session 在多步工作流中共享

**验证**：`cargo check` + `cargo test`

---

## 三、实施顺序与依赖关系

```
任务 1 (AgentHost) ──────────────────────────────── 独立，可先实施
任务 3 (消除静默错误吞没) ───────────────────────── 独立，可先实施
任务 4 (结构化日志) ─────────────────────────────── 独立，可先实施
任务 2 (IChatClient 管道重构) ───────────────────── 核心变更，依赖任务 3/4
  ├── 2.1 DelegatingChatClient
  ├── 2.2 ChatClientBuilder
  ├── 2.3 FunctionInvokingChatClient ← 迁移 ToolLoopAgent 逻辑
  ├── 2.4 PerServiceCallPersistingChatClient
  ├── 2.5 重构 ChatClientAgent + Builder
  └── 2.6 废弃 ToolLoopAgent
任务 5 (ProviderState<T>) ───────────────────────── 独立，可在任务 2 之后
任务 6 (记忆/向量检索) ──────────────────────────── 独立，可在任务 5 之后
任务 7 (ISession 多态) ──────────────────────────── 独立，可在任务 1 之后
任务 8 (工作流 Session) ─────────────────────────── 依赖任务 7
```

**推荐实施顺序**：1 → 3 → 4 → 2 → 5 → 7 → 8 → 6

---

## 四、文件变更清单

### 新建文件

| 文件路径 | 说明 |
|---------|------|
| `crates/framework/src/agent_host.rs` | AgentHost Session 注册中心 |
| `crates/framework/src/chat_client_decorators/mod.rs` | ChatClient 装饰器模块 |
| `crates/framework/src/chat_client_decorators/function_invoking.rs` | FunctionInvokingChatClient |
| `crates/framework/src/chat_client_decorators/per_service_call_persisting.rs` | PerServiceCallPersistingChatClient |
| `crates/core/src/vector_store.rs` | IVectorStore trait |
| `crates/framework/src/context_providers/memory_provider.rs` | MemoryContextProvider |

### 修改文件

| 文件路径 | 修改内容 |
|---------|---------|
| `crates/core/src/chat_client.rs` | 新增 DelegatingChatClient、ChatClientBuilder |
| `crates/core/src/session.rs` | 新增 ProviderState\<T\> |
| `crates/core/src/agent.rs` | 新增 create_session()、deserialize_session() |
| `crates/core/src/lib.rs` | 新增 mod 和 re-export |
| `crates/framework/src/lib.rs` | 新增 mod 和 re-export |
| `crates/framework/src/chat_client_agent.rs` | 使用 ChatClientBuilder 构建管道，移除 ToolLoopAgent 依赖 |
| `crates/framework/src/builder.rs` | 使用 ChatClientBuilder 替代 ToolLoopAgent 包装 |
| `crates/framework/src/agents/tool_loop_agent.rs` | 标记 deprecated |
| `crates/framework/src/context_providers/history_provider.rs` | 消除静默错误吞没 |
| `crates/workflow/src/patterns/*.rs` | Session 传递 |

---

## 五、验证步骤

1. **AgentHost 验证**：`get_or_create_session` 正确加载/创建，`run` 前后自动 save
2. **管道模式验证**：`FunctionInvokingChatClient` 正确执行工具循环，无重复数据
3. **PerServiceCall 持久化验证**：工具循环中途失败时，已完成轮次的消息已持久化
4. **静默错误消除验证**：grep `let _ =` 无残留（session 相关）
5. **会话隔离验证**：不同隔离键的 Session 互不可见
6. **压缩策略验证**：构造超过模型上下文窗口的长对话，验证自动压缩
7. **工作流 Session 验证**：Sequential 模式下多步 Agent 共享对话历史
8. **最终**：`cargo check` + `cargo test` + `cargo clippy` 全部通过

---

## 六、假设与决策

1. **决策**：ToolLoopAgent 按 MAF 管道模式重构，工具循环下沉到 IChatClient 层
2. **决策**：Session 注册中心由 AgentHost 提供，保持 `run()` 传入 Session 的签名
3. **决策**：会话隔离采用装饰器模式（IsolationScopedSessionStore 包装 ISessionStore）
4. **决策**：命名规范 ModelInfo → ModelMetadata，SessionTTLConfig → SessionTTLOptions
5. **决策**：改进方案保持 RAF 的 trait 抽象 + 管道模式设计哲学，用 Rust 惯用方式实现 MAF 的设计理念
6. **假设**：Rust 生态中存在可用的 tiktoken-rs 或等效 token 计数库
7. **假设**：向量数据库（如 Qdrant）有可用的 Rust SDK

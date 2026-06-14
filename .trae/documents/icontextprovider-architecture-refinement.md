# IContextProvider 架构重构计划（v2 — 整合 Session 改进 + 默认 Provider）

> **目标**：
> 1. `ContextProviderAgent` wrapper → 内化到 `ChatClientAgent`，对齐 MAF
> 2. `ISession` P1 改进（原子写入 + count + KV hash trait化）
> 3. `HistoryContextProvider` → `InMemoryHistoryProvider`，AgentBuilder 默认注册
> 4. Channel 分叉非阻塞 post-invocation，保留流式体验

---

## 一、问题总览

### 1.1 上一个计划的关键缺口

| 缺口 | 影响 |
|------|------|
| Session 无 `add_messages_batch` | HistoryProvider 逐条 `add_message`，中途崩溃状态不一致 |
| Session 无 `get_message_count` | `get_messages()` 全量克隆只为查 `.len()` |
| `touch_request_hash` 不在 `ISession` trait | KV 缓存追踪完全断链 |
| `last_message_count` 猜谜逻辑 | Provider 靠自己计数判断新消息，ToolLoopAgent 中途写消息会打破 |
| `HistoryContextProvider` 命名 | 与 MAF `InMemoryHistoryProvider` 不一致 |
| 用户必须手动注册 HistoryProvider | 不符合"默认内置"的体验预期 |

### 1.2 目标架构

```
ChatClientAgent { providers, tools }
  └── 内置默认: InMemoryHistoryProvider (AgentBuilder 自动注册)

ChatClientAgent.run():
  Phase 1: providers.on_invoking()  → 合并 instructions/messages/tools + KV hash
  Phase 2: LLM 调用                  → 流式响应
  Phase 3: [channel fork]            → 后台收集 → providers.on_invoked()
  
  return stream                      → 调用方实时收到 chunk
```

```
AgentBuilder.build():
  ChatClientAgent
    .with_instructions(...)
    .with_tools(...)  
    .with_context_providers([InMemoryHistoryProvider, ...])  ← 默认注入
  └── ToolLoopAgent wrapper (if tools)
```

---

## 二、变更清单

### 变更 A：ISession P1 改进（core crate）

**文件**：`crates/core/src/session.rs`（**修改**）

#### A1. ISession trait 新增方法

```rust
/// 原子批量追加消息（替代逐条 add_message）
/// 保证全部写入或全部不写入（Vec 级别原子性）
async fn add_messages_batch(&self, messages: &[ChatMessage]) -> Result<()> {
    // 默认实现：逐条 fallback
    for msg in messages {
        self.add_message(msg.clone()).await?;
    }
    Ok(())
}

/// 获取当前消息数量（零克隆，O(1)）
fn get_message_count(&self) -> usize {
    0  // 默认实现
}

/// 记录本次请求的 messages 哈希（用于 KV cache 前缀追踪）
/// 调用方（ChatClientAgent/ContextProviderAgent）在发送 LLM 请求前调用
fn touch_request_hash(&self, _messages: &[ChatMessage]) {
    // 默认空实现
}

/// 获取上次请求的 messages 哈希
/// 用于判断当前请求前缀是否命中上次的 KV 缓存
fn get_last_request_hash(&self) -> Option<u64> {
    None
}
```

#### A2. AgentSession 实现

```rust
async fn add_messages_batch(&self, messages: &[ChatMessage]) -> Result<()> {
    let mut history = self.history.write().await;
    history.extend(messages.iter().cloned());
    let count = messages.len() as u64;
    drop(history);
    let mut meta = self.metadata.write().await;
    meta.message_count += count;
    meta.updated_at = Utc::now();
    Ok(())
}

fn get_message_count(&self) -> usize {
    self.history.blocking_read().len()
}
```

（`touch_request_hash` / `get_last_request_hash` 已存在于 `AgentSession`，仅需从 trait 入口暴露）

### 变更 B：重命名 HistoryContextProvider → InMemoryHistoryProvider

**文件**：`crates/framework/src/context_providers/history_provider.rs`（**重构**）

- 结构体：`HistoryContextProvider` → `InMemoryHistoryProvider`
- `name()` 返回值：`"HistoryContextProvider"` → `"InMemoryHistoryProvider"`
- 更新所有文档注释，对齐 MAF `InMemoryHistoryProvider` 语义

#### B1. 重写 on_invoked() —— 使用新 Session API 消除猜谜

```rust
async fn on_invoked(
    &self,
    _agent: &dyn IAgent,
    session: &dyn ISession,
    request_messages: &[ChatMessage],
    response: Option<&AgentResponse>,
    _error: Option<&rust_agent_core::AgentError>,
) -> Result<()> {
    // 确定哪些是新增的 user 消息（request_messages 中新于 session 当前计数的）
    let existing_count = session.get_message_count();
    let system_count = request_messages
        .iter()
        .filter(|m| m.role == MessageRole::System)
        .count();
    
    // 收集本轮新增的 caller messages + assistant response
    let mut new_messages = Vec::new();

    // 新增 user/tool 消息
    let new_start = system_count.saturating_add(existing_count);
    if new_start < request_messages.len() {
        for msg in &request_messages[new_start..] {
            if msg.role != MessageRole::System {
                new_messages.push(msg.clone());
            }
        }
    }

    // Assistant 响应文本
    if let Some(resp) = response {
        if !resp.text.is_empty() {
            new_messages.push(ChatMessage::assistant(resp.text.clone()));
        }
    }

    // 原子批量写入
    if !new_messages.is_empty() {
        let _ = session.add_messages_batch(&new_messages).await;
    }

    Ok(())
}
```

**关键改进**：
- 不再依赖 `provider_state["last_message_count"]` —— 直接用 `session.get_message_count()` 获取实时计数
- 不再二次 `get_messages()` 只为查 count
- 用 `add_messages_batch` 原子写入，消除部分持久化风险

### 变更 C：ChatClientAgent 集成 IContextProvider + Session KV 追踪

**文件**：`crates/framework/src/chat_client_agent.rs`（**重构**）

#### C1. 结构体变更

```rust
pub struct ChatClientAgent {
    id: AgentId,
    metadata: AgentMetadata,
    chat_client: Arc<dyn IChatClient>,
    instructions: String,
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    context_providers: Vec<Arc<dyn IContextProvider>>,  // 新增
}
```

新增构造方法：

```rust
pub fn with_context_providers(
    mut self,
    providers: Vec<Arc<dyn IContextProvider>>,
) -> Self {
    self.context_providers = providers;
    self
}
```

#### C2. run() 方法重构

完整流程：

```
Phase 1: Pre-invocation
  ├── for provider in context_providers:
  │     injection = provider.on_invoking(self, session, messages, options)
  │     merge(instructions, messages, tools)
  ├── 组装 [system + merged_instructions] + [provider_messages] + [caller_messages]
  └── session.touch_request_hash(full_messages)     ← KV 缓存追踪
     
Phase 2: LLM 调用 + 流转换
  └── IChatClient.run() → AgentResponseConverter → AgentResponseResult stream

Phase 3: Channel 分叉（仅当有 providers 时）
  ├── mpsc::unbounded_channel 复制流
  ├── 主流: stream.inspect(channel.send) → return to caller (实时)
  └── 后台 spawn:
       ├── collect from channel
       ├── aggregate AgentResponse
       └── for provider in context_providers:
             provider.on_invoked(proxy, session, request_messages, response, None)
```

#### C3. AgentProxy

spawn 中无法捕获 `&self`，使用轻量 proxy 传递 agent id/metadata 给 `on_invoked`：

```rust
struct AgentProxy {
    id: AgentId,
    metadata: AgentMetadata,
}

#[async_trait]
impl IAgent for AgentProxy {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }
    // run() → Err (post-invocation 不应调用 run)
    // get_subagent / list_subagents → 空
}
```

### 变更 D：删除 ContextProviderAgent + 更新导出

**文件**：`crates/framework/src/agents/context_provider_agent.rs`（**删除**）
**文件**：`crates/framework/src/agents/mod.rs`（**修改**）

```rust
// 移除 pub mod context_provider_agent;
pub mod tool_loop_agent;
```

**文件**：`crates/framework/src/lib.rs`（**修改**）

```rust
// 移除: pub use agents::context_provider_agent::ContextProviderAgent;
// 重命名: HistoryContextProvider → InMemoryHistoryProvider
```

### 变更 E：AgentBuilder 扁平化 + 默认 InMemoryHistoryProvider

**文件**：`crates/framework/src/builder.rs`（**修改**）

```rust
pub fn build(self) -> Result<Arc<dyn IAgent>> {
    // ── 确定 context_providers ──────────────────────────────────
    let context_providers = if self.context_providers.is_empty() {
        // 默认注入 InMemoryHistoryProvider（对标 MAF 行为）
        vec![Arc::new(InMemoryHistoryProvider::new()) as Arc<dyn IContextProvider>]
    } else {
        self.context_providers
    };

    // ── 1. ChatClientAgent ──────────────────────────────────────
    let mut agent = ChatClientAgent::new(&self.agent_id, Arc::new(chat_client))
        .with_instructions(&self.instructions)
        .with_context_providers(context_providers);

    if !self.description.is_empty() {
        agent = agent.with_description(&self.description);
    }
    if !self.tools.is_empty() {
        let mut registry = ToolRegistry::new();
        for t in &self.tools {
            registry.register_arc(Arc::clone(t));
        }
        agent = agent.with_tools(registry);
    }

    let agent: Arc<dyn IAgent> = Arc::new(agent);

    // ── 2. ToolLoopAgent wrapper ─────────────────────────────────
    let agent: Arc<dyn IAgent> = if !self.tools.is_empty() {
        Arc::new(
            ToolLoopAgent::new(
                format!("{}-tool-loop", self.agent_id),
                agent,
                self.tools,
            )
            .with_max_rounds(self.max_tool_rounds),
        )
    } else {
        agent
    };

    Ok(agent)
}
```

**设计要点**：
- 默认注入 `InMemoryHistoryProvider::new()`，用户无需手动注册
- 用户显式调用 `.with_context_provider(MyProvider)` 时**完全覆盖**默认（不合并）
- 这样可以替换默认 provider 或添加额外 provider

### 变更 F：core 层导出更新

**文件**：`crates/core/src/lib.rs`（**修改**）

```rust
pub use session::{
    AgentSession, ISession, ProviderStateStore, SessionMetadata, SessionSnapshot,
};
```

**文件**：`crates/framework/src/lib.rs`（**修改**）

```rust
pub use context_providers::history_provider::InMemoryHistoryProvider;
```

### 变更 G：重命名文件

**文件**：`crates/framework/src/context_providers/history_provider.rs` → 保留文件名（或用新名 `memory_provider.rs`，但考虑到结构体已重命名，文件名可保持不变以减少 diff）

---

## 三、文件变更汇总

| 操作 | 文件 | 说明 |
|------|------|------|
| **重构** | `crates/framework/src/chat_client_agent.rs` | 集成 providers + Session KV hash + channel 分叉 |
| **删除** | `crates/framework/src/agents/context_provider_agent.rs` | 不再需要 |
| **重构** | `crates/framework/src/context_providers/history_provider.rs` | 重命名为 InMemoryHistoryProvider + 用新 API 消除猜谜 |
| **修改** | `crates/core/src/session.rs` | P1: add_messages_batch, get_message_count, touch_request_hash, get_last_request_hash 提升到 ISession trait |
| **修改** | `crates/core/src/lib.rs` | 导出新 Session 类型 |
| **修改** | `crates/framework/src/lib.rs` | 移除 ContextProviderAgent，导出 InMemoryHistoryProvider |
| **修改** | `crates/framework/src/agents/mod.rs` | 移除 context_provider_agent 模块 |
| **修改** | `crates/framework/src/builder.rs` | 扁平化 + 默认注入 InMemoryHistoryProvider |
| **不变** | `crates/core/src/context_provider.rs` | IContextProvider trait 不变 |

---

## 四、架构决策记录 (ADR)

### ADR-001: Provider 内化到 ChatClientAgent

同前版。

### ADR-002: Channel 分叉非阻塞 post-invocation

同前版。

### ADR-003: AgentProxy 模式

同前版。

### ADR-004: ToolLoopAgent 保留 wrapper

同前版。

### ADR-005: AgentBuilder 默认注入 InMemoryHistoryProvider

**决策**：`AgentBuilder.build()` 始终注入 `InMemoryHistoryProvider`，除非用户显式指定 `.with_context_provider()`（完全替换默认）。

**理由**：
1. 对齐 MAF — MAF Python SDK 在 `RawAgent` 中可能自动添加 `InMemoryHistoryProvider`
2. 降低用户入门门槛 — 不需要了解 Provider 概念即可获得多轮对话能力
3. 用户显式调用 `.with_context_provider()` 时完全覆盖，保持可定制性

### ADR-006: ISession 新增方法均带默认实现

**决策**：`add_messages_batch`、`get_message_count`、`touch_request_hash`、`get_last_request_hash` 均提供默认实现。

**理由**：
1. 不破坏现有 `ISession` 实现者
2. `get_message_count` 默认返回 0（退化到每次都全量计算）
3. `add_messages_batch` 默认逐条 fallback

### ADR-007: InMemoryHistoryProvider 不再使用 provider_state 跟踪计数

**决策**：`on_invoked()` 使用 `session.get_message_count()` 实时获取计数，不再在 `provider_state` 中维护 `last_message_count`。

**理由**：
1. `get_message_count()` 是 O(1) 操作（`Vec::len`），无需额外缓存
2. 消除 `provider_state` 与实际 state 不同步的风险
3. 简化 Provider 逻辑

---

## 五、验证方案

### 5.1 编译

```bash
cargo build --workspace
cargo clippy --workspace
cargo test --workspace
```

### 5.2 功能场景

| 场景 | 验证点 |
|------|--------|
| **默认自动注册** | 不调用 `.with_context_provider()`，直接 `.build()` → 多轮对话正常 |
| **默认被覆盖** | `.with_context_provider(MyProvider)` → 默认 InMemoryHistoryProvider 被替换 |
| **多 Provider** | `.with_context_provider(A).with_context_provider(B)` → 链式执行顺序正确 |
| **流式不阻塞** | 首个 token 到达 ≈ API 首个 token（无额外延迟） |
| **KV hash 追踪** | `touch_request_hash` 被调用，`get_last_request_hash` 可查询 |
| **批量写入原子性** | `add_messages_batch` 单次写入多条消息 |
| **Session 序列化** | provider_states + messages 正确恢复 |

---

## 六、风险

| 风险 | 缓解 |
|------|------|
| `inspect` 闭包中 clone 开销 | 每 chunk 一次 clone，轻量 |
| ToolLoopAgent 双重写 session | InMemoryHistoryProvider 通过 `get_message_count` 实时检测，避免重复写入 |
| 默认注入行为变更 | 显式文档说明 + `.with_context_provider()` 覆盖机制 |

# 完备流式响应模型 Spec

## 需求复述

1. **`AgentResponseResult` 作为流式输出载体**：`IAgent::run()` → `BoxStream<AgentResponseResult>`，每个 chunk 携带 `contents`（内容变体数组）+ `events`（事件变体数组）
2. **`ResponseMetadata` 统一元数据**：每个 content/event 变体自带 `ResponseMetadata { agent_id, model_id, executor_id, timestamp, properties }`，其中 `properties` 由 `AgentRunOptions.properties` 透传
3. **内容变体 7 种**：`TextContent` / `ReasoningContent` / `UriContent` / `ToolCallingContent`(工具开始→触发 execute) / `ToolCalledContent`(工具结束→result/error) / `UsageContent` / `ErrorContent`
4. **事件变体可扩展**：`ExecutorInvokingEvent` / `ExecutorInvokedEvent` / `CustomEvent`
5. **IAgent 统一门面**：`AgentBuilder.build() → IAgent`、`WorkflowBuilder → IAgent`、`get_subagent(agent_id) → IAgent`，调用体验完全一致
6. **内部转换链路分层**：SSE 原始数据 → `AgentResponseUpdate`(transport 层) → `AgentResponseConverter`(framework 层) → `AgentResponseResult`(public API)
7. **多轮对话 / KV 缓存 / Session 完整管理**同前

---

## Design Goals

| 目标 | 说明 |
|------|------|
| **分层架构** | Transport(client) → Converter(framework) → Public(core)，每层职责清晰 |
| **元数据透传** | `ResponseMetadata` 贯穿全链路，`properties` 从 `AgentRunOptions` 透传至每个 content/event |
| **工具两阶段** | `ToolCallingContent`(触发点) → `ITool.execute` → `ToolCalledContent`(含 result/error) |
| **门面统一** | `AgentBuilder` / `ToolLoop` / `Workflow` 均产出 `Arc<dyn IAgent>` |
| **并行安全** | 每个变体自带 `ResponseMetadata`，交叉输出可追溯到源 agent/executor |

---

## 架构总览（内部转换链路）

```
┌─────────────────────────────────────────────────────────────────────┐
│  Layer 0: Core Types (core crate)                                    │
│  ResponseMetadata, Content/Event enums, AgentResponseResult, IAgent, │
│  AgentRunOptions(含 properties), ChatMessage, AgentResponse, Usage   │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 1: Transport (client crate)                                   │
│                                                                      │
│  reqwest byte stream                                                 │
│    │                                                                 │
│    ▼  SseStream::poll_next()                                        │
│  raw SSE line ──parse──▶ SseChunk ──map_chunk()──▶ AgentResponseUpdate │
│                            (内部类型，不对外暴露)                       │
│                                                                      │
│  AgentResponseUpdate variants:                                       │
│    TextDelta | ReasoningDelta | ToolCallDelta | Usage | Finish       │
│    Error | ResponseMetadata { id, model }                            │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 2: Conversion (framework crate)                               │
│                                                                      │
│  AgentResponseConverter                                              │
│    │                                                                 │
│    │  输入: Stream<AgentResponseUpdate>     (transport 层产出)        │
│    │  上下文: &AgentRunOptions, AgentMetadata                         │
│    │  输出: Stream<AgentResponseResult>      (公共 API)               │
│    │                                                                 │
│    │  转换规则:                                                       │
│    │  ┌─────────────────────┬──────────────────────────────────┐    │
│    │  │ TextDelta           │ → Content::Text(TextContent)     │    │
│    │  │ ReasoningDelta      │ → Content::Reasoning(..)         │    │
│    │  │ ToolCallDelta       │ → 累积到 accumulator             │    │
│    │  │   (args 完整时)     │ → Content::ToolCalling(..)       │    │
│    │  │ Usage               │ → Content::Usage(UsageContent)   │    │
│    │  │ Finish              │ → AgentResponseResult.finish_reason │
│    │  │ ResponseMetadata    │ → AgentResponseResult.id/model   │    │
│    │  │ Error               │ → Content::Error(ErrorContent)   │    │
│    │  └─────────────────────┴──────────────────────────────────┘    │
│    │                                                                 │
│    │  每个 Content/Event 附加 ResponseMetadata:                       │
│    │    agent_id    ← AgentMetadata.key / IAgent::id()               │
│    │    model_id    ← AgentResponseUpdate::ResponseMetadata.model    │
│    │    executor_id ← 当前 executor 标识（单 Agent 时 = agent_id）     │
│    │    timestamp   ← Utc::now()                                     │
│    │    properties  ← AgentRunOptions.properties                     │
│    │                                                                 │
├─────────────────────────────────────────────────────────────────────┤
│  Layer 3: Agent Pipeline (framework crate)                           │
│                                                                      │
│  ChatClientAgent                                                     │
│    run() → 拼装 messages + session → IChatClient::run()              │
│    → Stream<AgentResponseUpdate> → AgentResponseConverter           │
│    → Stream<AgentResponseResult>                                     │
│                                                                      │
│  ToolLoopAgent (wraps inner IAgent)                                  │
│    run() → inner.run() → Stream<AgentResponseResult>                │
│    拦截 ToolCallingContent → ITool.execute()                         │
│    → 注入 ToolCalledContent { call_id, result/error }               │
│    → 构造 ChatMessage::tool() → 再次 inner.run()                    │
│    → 循环直到 Finish(Stop) 或 max_rounds                             │
│                                                                      │
│  HistoryAgent (wraps inner IAgent)                                   │
│    run() → ISession::get_messages() 加载历史                         │
│    → 拼装 [system] + history + messages → inner.run()               │
│    → 流结束后 ISession::add_message() 持久化                         │
│                                                                      │
│  TracingAgent (wraps inner IAgent)                                   │
│    run() → tracing::info_span! 记录耗时 / token / 错误               │
│    → 同时产出 Event::ExecutorInvoking / ExecutorInvoked              │
├─────────────────────────────────────────────────────────────────────┤
│  Public API                                                          │
│                                                                      │
│  let agent = AgentBuilder::new("my-agent")                           │
│      .chat_client(client)                                            │
│      .instructions("...")                                            │
│      .with_tool(my_tool)                                             │
│      .with_properties([("tenant", "acme")])                          │
│      .build()?;                    // → Arc<dyn IAgent>              │
│                                                                      │
│  let stream = agent.run(messages, session, options).await?;          │
│  // → BoxStream<AgentResponseResult>                                 │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 调用示例代码

### 1. 流式消费 + ResponseMetadata 追踪

```rust
use rust_agent_core::*;
use futures_util::StreamExt;

let agent: Arc<dyn IAgent> = AgentBuilder::new("assistant")
    .chat_client(deepseek_client)
    .instructions("你是一位助手")
    .with_properties([("source", "web-ui".into()), ("tenant", "acme".into())])
    .build()?;

let stream = agent.run(messages, Some(session.clone()), None).await?;

while let Some(result) = stream.next().await {
    let chunk = result?;

    for content in &chunk.contents {
        // 每个 content 都带 ResponseMetadata
        let meta = content.meta();

        match content {
            Content::Text(c) => {
                // meta.agent_id    → "assistant"
                // meta.model_id    → "deepseek-chat"
                // meta.properties  → {"source":"web-ui","tenant":"acme"}
                print!("{}", c.delta);
            }
            Content::ToolCalling(c) => {
                // ToolCallingContent 是 ITool.execute 的触发点
                println!("[调用] agent={} tool={} args={}",
                    meta.agent_id.as_deref().unwrap_or("?"),
                    c.name, c.arguments);
            }
            Content::ToolCalled(c) => {
                if let Some(err) = &c.error {
                    eprintln!("[工具错误] call_id={} error={}", c.call_id, err);
                } else {
                    println!("[工具结果] call_id={} result={}",
                        c.call_id, c.result.as_deref().unwrap_or(""));
                }
            }
            Content::Usage(c) => {
                let hit = c.usage.prompt_cache_hit_tokens.unwrap_or(0);
                let miss = c.usage.prompt_cache_miss_tokens.unwrap_or(0);
                println!("[用量] tokens={} cache_hit={}/{} properties={:?}",
                    c.usage.total_tokens, hit, hit + miss, meta.properties);
            }
            _ => {}
        }
    }

    for event in &chunk.events {
        match event {
            Event::ExecutorInvoking(e) =>
                println!("[调度] {} 开始", e.executor_id),
            Event::ExecutorInvoked(e) =>
                println!("[调度] {} 完成 {}ms", e.executor_id, e.duration_ms),
            _ => {}
        }
    }
}
```

### 2. AgentBuilder → IAgent（统一入口）

```rust
// 方式 1: 简单 Agent
let agent1: Arc<dyn IAgent> = AgentBuilder::new("simple")
    .chat_client(deepseek_client)
    .build()?;

// 方式 2: 带工具 + 属性透传
let agent2: Arc<dyn IAgent> = AgentBuilder::new("tool-agent")
    .chat_client(deepseek_client)
    .instructions("你是天气助手")
    .with_tool(get_weather_tool)
    .with_tool(get_time_tool)
    .with_properties([("department", "weather")])
    .build()?;

// 方式 3: Workflow 也是 IAgent
let agent3: Arc<dyn IAgent> = WorkflowBuilder::new(/* ... */).build()?;

// 三种方式调体验完全一致
let stream1 = agent1.run(messages, session, None).await?;
let stream2 = agent2.run(messages, session, None).await?;
let stream3 = agent3.run(messages, session, None).await?;
```

### 3. 多轮对话 — Session 自动管理上下文

```rust
// Agent 构建时已配置 system instructions
let agent: Arc<dyn IAgent> = AgentBuilder::new("assistant")
    .chat_client(deepseek_client)
    .instructions("你是一位乐于助人的助手")  // ← system message 在构建时设置
    .build()?;

let session = Arc::new(AgentSession::new());

// ── Turn 1 ──
// 调用方只需传用户问题，系统自动注入 instructions + session history
let resp1 = collect_agent_response(
    agent.run(
        vec![ChatMessage::user("北京的首都是？")],
        Some(session.clone()),
        None,
    ).await?
).await?;
println!("第1轮: {}", resp1.text);
// 框架内部实际发送: [system(instructions), user:"北京的首都是？"]
// Session 自动追加: user:"北京的首都是？", assistant:"北京是中国的首都。"

// ── Turn 2 ──
// 只发送用户问题，框架自动加载 session 历史拼装完整上下文
let resp2 = collect_agent_response(
    agent.run(
        vec![ChatMessage::user("上海的首都是？")],
        Some(session.clone()),
        None,
    ).await?
).await?;
// 框架内部实际发送: [system, user:"北京...", assistant:"北京是...", user:"上海..."]
// 前缀完整匹配 Turn1 → prompt_cache_hit_tokens > 0
println!("第2轮: {}", resp2.text);

// ── Turn 3 ──
let resp3 = collect_agent_response(
    agent.run(
        vec![ChatMessage::user("深圳的呢？")],
        Some(session.clone()),
        None,
    ).await?
).await?;
// 框架内部实际发送: [system, user:"北京...", assistant:"北京...", user:"上海...", assistant:"上海...", user:"深圳..."]
// 前缀稳定递增 → 每轮都命中 KV 缓存
```
**调用方契约**：每轮只需传 `vec![ChatMessage::user("问题")]`，system 和 history 由框架自动管理。

---

## 类型定义

### ResponseMetadata（原 ContentMeta，扩展 properties）

```rust
/// 每个 content / event 变体自带的统一元数据
/// properties 由 AgentRunOptions.properties 透传
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub agent_id: Option<AgentId>,
    pub model_id: Option<String>,
    pub executor_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    /// 透传参数 — 来自 AgentRunOptions.properties
    pub properties: HashMap<String, serde_json::Value>,
}
```

### Content 枚举 + 各变体

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Content {
    Text(TextContent),
    Reasoning(ReasoningContent),
    Uri(UriContent),
    ToolCalling(ToolCallingContent),
    ToolCalled(ToolCalledContent),
    Usage(UsageContent),
    Error(ErrorContent),
}

pub trait HasMeta { fn meta(&self) -> &ResponseMetadata; }

pub struct TextContent       { pub meta: ResponseMetadata, pub delta: String }
pub struct ReasoningContent  { pub meta: ResponseMetadata, pub delta: String }
pub struct UriContent        { pub meta: ResponseMetadata, pub uri: String, pub label: Option<String> }
pub struct ToolCallingContent { pub meta: ResponseMetadata, pub call_id: String, pub name: String, pub arguments: serde_json::Value }
pub struct ToolCalledContent { pub meta: ResponseMetadata, pub call_id: String, pub result: Option<String>, pub error: Option<String> }
pub struct UsageContent      { pub meta: ResponseMetadata, pub usage: Usage }
pub struct ErrorContent      { pub meta: ResponseMetadata, pub error_code: String, pub message: String }
```

### Event 枚举（meta 统一为 ResponseMetadata）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    ExecutorInvoking(ExecutorInvokingEvent),
    ExecutorInvoked(ExecutorInvokedEvent),
    Custom(CustomEvent),
}

pub struct ExecutorInvokingEvent { pub meta: ResponseMetadata, pub executor_id: String, pub executor_type: String, pub input_message_count: usize }
pub struct ExecutorInvokedEvent  { pub meta: ResponseMetadata, pub executor_id: String, pub duration_ms: u64, pub output_content_count: usize }
pub struct CustomEvent           { pub meta: ResponseMetadata, pub event_type: String, pub payload: serde_json::Value }
```

### AgentResponseResult

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponseResult {
    pub id: Option<String>,
    pub model: Option<String>,
    pub finish_reason: Option<FinishReason>,
    pub contents: Vec<Content>,
    pub events: Vec<Event>,
}
```

### AgentRunOptions（新增 properties）

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRunOptions {
    pub instructions: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    pub extra_body: HashMap<String, serde_json::Value>,
    /// 透传属性 — 附加到每个 content/event 的 ResponseMetadata.properties
    pub properties: HashMap<String, serde_json::Value>,
}
```

### IAgent trait

```rust
#[async_trait]
pub trait IAgent: Send + Sync {
    fn id(&self) -> &AgentId;
    fn metadata(&self) -> &AgentMetadata;
    async fn run(&self, messages: Vec<ChatMessage>, session: Option<Arc<dyn ISession>>, options: Option<AgentRunOptions>) -> BoxStream<'static, Result<AgentResponseResult>>;
    fn get_subagent(&self, agent_id: &AgentId) -> Option<Arc<dyn IAgent>>;
    fn list_subagents(&self) -> Vec<Arc<dyn IAgent>>;
    async fn reset(&self);
}
```

### AgentResponseUpdate（内部私有类型 — client crate）

```rust
// pub(crate) — 仅 client crate + framework crate 可见
pub(crate) enum AgentResponseUpdate {
    TextDelta          { delta: String },
    ReasoningDelta     { delta: String },
    ToolCallDelta      { index: usize, id: Option<String>, name: Option<String>, arguments_delta: String },
    Usage              { usage: Usage },
    Finish             { finish_reason: FinishReason, usage: Option<Usage> },
    Error              { message: String },
    ResponseMetadata   { id: Option<String>, model: Option<String> },
}
```

### AgentResponseConverter（framework crate — 核心转换器）

```rust
/// AgentResponseUpdate → AgentResponseResult 转换器
/// 职责：累积增量、附加 ResponseMetadata、产出公共 API 类型
pub struct AgentResponseConverter {
    agent_id: AgentId,
    model_id: Option<String>,
    executor_id: String,
    properties: HashMap<String, serde_json::Value>,
    // 内部状态
    tool_call_accumulator: HashMap<usize, ToolCallAccumulator>,
    response_id: Option<String>,
    response_model: Option<String>,
}

impl AgentResponseConverter {
    pub fn new(
        agent_id: AgentId,
        executor_id: String,
        options: &AgentRunOptions,
    ) -> Self;

    /// 从 options 构造默认 properties
    fn build_meta(&self) -> ResponseMetadata;

    /// 消费单个 AgentResponseUpdate，产出 Vec<Content> + Vec<Event>
    pub fn consume(&mut self, update: AgentResponseUpdate) -> ConvertOutput;

    /// 流结束时产出最终 AgentResponseResult（含 finish_reason + usage）
    pub fn finalize(&mut self, finish_reason: Option<FinishReason>, usage: Option<Usage>) -> AgentResponseResult;
}

pub struct ConvertOutput {
    pub contents: Vec<Content>,
    pub events: Vec<Event>,
}
```

### AgentBuilder（framework crate）

```rust
pub struct AgentBuilder<C> {
    agent_id: AgentId,
    chat_client: C,
    instructions: Option<String>,
    tools: Vec<Arc<dyn ITool>>,
    properties: HashMap<String, serde_json::Value>,
    // ... other fields
}

impl<C: IChatClient + 'static> AgentBuilder<C> {
    pub fn new(id: impl Into<String>) -> Self;
    pub fn chat_client(mut self, client: C) -> Self;
    pub fn instructions(mut self, text: impl Into<String>) -> Self;
    pub fn with_tool(mut self, tool: impl ITool + 'static) -> Self;
    pub fn with_properties(mut self, iter: impl IntoIterator<Item = (String, serde_json::Value)>) -> Self;

    /// 构建最终 Agent 管道
    /// 内部组装: TracingAgent(ToolLoopAgent(HistoryAgent(ChatClientAgent)))
    /// → 返回 Arc<dyn IAgent>
    pub fn build(self) -> Result<Arc<dyn IAgent>>;
}
```

### ChatMessage（扩展后）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}
```

序列化规则：
| 消息角色 | JSON 输出 | 关键字段 |
|---------|----------|---------|
| `System` | `{"role":"system","content":"..."}` | `content` |
| `User` | `{"role":"user","content":"..."}` | `content` |
| `Assistant`(文本) | `{"role":"assistant","content":"..."}` | `content` |
| `Assistant`(工具调用) | `{"role":"assistant","content":"","tool_calls":[...]}` | `tool_calls` |
| `Tool` | `{"role":"tool","content":"...","tool_call_id":"call_xxx"}` | `content`, `tool_call_id` |

### Usage（core crate）

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub prompt_cache_hit_tokens: Option<u32>,
    pub prompt_cache_miss_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
}
```

### FinishReason（core crate）

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    #[serde(untagged)]
    Other(String),
}
```

### AgentResponse（聚合结果）

```rust
/// 对标 MAF AgentResponse，由 collect_agent_response() 产出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub text: String,
    pub reasoning_text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<FinishReason>,
    pub usage: Option<Usage>,
    pub source_agent_id: Option<AgentId>,
}
```

---

## 完整 Session 管理

### 一、Session 在整体架构中的位置

```
调用方
  │ agent.run(messages, session, options)
  ▼
┌───────────────────────────────────────────────────────────────────┐
│  HistoryAgent (wraps inner IAgent)                                │
│                                                                   │
│  执行顺序：                                                        │
│  1. 从 ISession 加载历史消息                                       │
│  2. 拼装完整消息列表:                                              │
│     [system] + session.get_messages() + [new_user_message]         │
│  3. inner.run(full_messages, session, options)                    │
│  4. 流结束后，将 new_user_message 和 assistant_response 写入 session │
│  5. 更新 session.last_request_hash                                │
└───────────────────────────────────────────────────────────────────┘
```

### 二、ISession trait 定义（core crate）

```rust
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Session — 对标 MAF AgentSession，管理多轮对话的消息生命周期
#[async_trait]
pub trait ISession: Send + Sync {
    /// 会话唯一标识
    fn session_id(&self) -> &str;

    /// 追加一条消息到会话历史
    async fn add_message(&self, message: ChatMessage) -> Result<()>;

    /// 获取所有历史消息（按时间顺序）
    async fn get_messages(&self) -> Result<Vec<ChatMessage>>;

    /// 清空会话历史
    async fn clear(&self) -> Result<()>;

    // ── 新增方法 ──

    /// 元数据（创建时间、消息数、缓存追踪哈希）
    fn metadata(&self) -> &SessionMetadata;

    /// 只读快照：包含 session_id + metadata + 当前所有消息
    fn snapshot(&self) -> SessionSnapshot;

    /// 序列化为 JSON 字符串（用于持久化）
    fn serialize(&self) -> Result<String>;

    /// 从 JSON 字符串反序列化恢复
    fn deserialize(data: &str) -> Result<Self> where Self: Sized;
}
```

### 三、SessionMetadata / SessionSnapshot

```rust
/// 会话元数据 — 不可变的历史统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// 会话创建时间（AgentSession::new() 时设置）
    pub created_at: DateTime<Utc>,
    /// 最后追加消息时间（add_message() 时自动更新）
    pub updated_at: DateTime<Utc>,
    /// 当前消息总数
    pub message_count: u64,
    /// 上次请求的 messages 列表内容哈希（用于 KV cache 前缀追踪）
    ///
    /// Hash = xxhash::xxh64( system.content + user.content + assistant.content + ... )
    /// 仅对 messages 数组的内容做哈希，不包含时间戳等可变字段
    pub last_request_hash: Option<u64>,
}

/// 只读会话快照 — 用于调试、UI 展示、审计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub metadata: SessionMetadata,
    pub messages: Vec<ChatMessage>,
}
```

### 四、AgentSession 实现（core crate）

```rust
pub struct AgentSession {
    session_id: String,
    history: RwLock<Vec<ChatMessage>>,
    metadata: RwLock<SessionMetadata>,
}

impl AgentSession {
    /// 创建新会话（自动生成 session_id，不依赖全局计数器）
    pub fn new() -> Self {
        Self {
            session_id: Uuid::new_v4().to_string(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: Utc::now(),
                updated_at: Utc::now(),
                message_count: 0,
                last_request_hash: None,
            }),
        }
    }

    /// 指定 session_id 创建（用于恢复持久化的会话）
    pub fn with_id(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            history: RwLock::new(Vec::new()),
            metadata: RwLock::new(SessionMetadata {
                created_at: Utc::now(),
                updated_at: Utc::now(),
                message_count: 0,
                last_request_hash: None,
            }),
        }
    }

    /// 记录本次请求的 messages 哈希（HistoryAgent 在发送 LLM 请求前调用）
    pub fn touch_request_hash(&self, messages: &[ChatMessage]) {
        let hash = hash_messages(messages);
        let mut meta = self.metadata.write();
        meta.last_request_hash = Some(hash);
    }
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

    fn metadata(&self) -> &SessionMetadata {
        // RwLock::read + 借用问题 → 通过 snapshot() 获取
        unimplemented!("使用 snapshot().metadata 代替")
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
        serde_json::to_string(&self.snapshot()).map_err(|e| AgentError::Serialize(e.to_string()))
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
```

### 五、Session 序列化格式

```json
{
    "session_id": "a1b2c3d4-...",
    "metadata": {
        "created_at": "2026-06-14T10:00:00Z",
        "updated_at": "2026-06-14T10:05:30Z",
        "message_count": 4,
        "last_request_hash": 17456789012345678901
    },
    "messages": [
        {"role":"system","content":"你是一位助手"},
        {"role":"user","content":"北京的首都是？"},
        {"role":"assistant","content":"北京是中国的首都。","tool_calls":null,"tool_call_id":null},
        {"role":"user","content":"上海的呢？"}
    ]
}
```

### 六、多轮对话生命周期

```
Time ──────────────────────────────────────────────────────────────►

 Turn 1: 创建 session, 调用方只传: [ChatMessage::user("北京?")]
   │     agent.run([user("北京?")], session)
   │     
   │     HistoryAgent 内部:
   │       session.get_messages() → []
   │       system_msg = ChatMessage::system(agent.instructions)
   │       拼装: [system("你是助手"), user("北京?")]
   │       ChatClientAgent.run(...) → LLM
   │       session.add_message(user("北京?"))
   │       session.add_message(assistant("北京是中国的首都。"))
   │       session.touch_request_hash([system, user, assistant])
   │     
   ▼     
 Turn 2: 调用方只传: [ChatMessage::user("上海?")]
   │     agent.run([user("上海?")], session)
   │     
   │     HistoryAgent 内部:
   │       session.get_messages() → [user("北京?"), assistant("北京是...")]
   │       system_msg = ChatMessage::system(agent.instructions)
   │       拼装: [system, user("北京?"), assistant("北京是..."), user("上海?")]
   │       ↑ 前缀 [system, user("北京?"), assistant("北京是...")] 命中 DeepSeek KV cache
   │       ChatClientAgent.run(...) → LLM (prompt_cache_hit_tokens > 0)
   │       session.add_message(user("上海?"))
   │       session.add_message(assistant("上海是中国的直辖市。"))
   │     
   ▼     
 Turn 3: 调用方只传: [ChatMessage::user("深圳?")]
   │     agent.run([user("深圳?")], session)
   │     框架内部: [system, user1, asst1, user2, asst2, user3] → cache hit
   │     
   ▼     
 Turn N: [serialize → 持久化]
   │     let json = session.serialize().unwrap();
   │     std::fs::write("session.json", &json).unwrap();
   │
   ▼
 恢复:
         let json = std::fs::read_to_string("session.json").unwrap();
         let session = AgentSession::deserialize(&json).unwrap();
         agent.run([user("广州?")], session)  // 继续对话，缓存继续命中
```

### 七、KV 缓存命中保障规则

DeepSeek KV 缓存要求：后续请求 `messages` 必须是之前请求的**前缀完整匹配**。

| 规则 | 说明 |
|------|------|
| **system 消息不变** | `system` 始终在最前，内容不变，否则缓存失效 |
| **前缀严格递增** | 每轮只追加 user + assistant，不删不改中间的 message |
| **拼装顺序固定** | `[system] + session_messages + [new_user_message]` — 不做重排 |
| **工具消息安全追加** | tool_calls 场景: `assistant(tool_calls) + tool(results)` 也按顺序追加 |
| **无中间修改** | 历史消息的内容一旦写入 session 就不修改 |

**违反任一条，KV 缓存命中率为 0。**

### 八、HistoryAgent 与 Session 交互契约

```rust
/// HistoryAgent 的 run() 伪代码
impl IAgent for HistoryAgent {
    async fn run(&self, messages: Vec<ChatMessage>, session: Option<Arc<dyn ISession>>, options: Option<AgentRunOptions>) -> BoxStream<'static, Result<AgentResponseResult>> {
        let session = session.unwrap_or_else(|| Arc::new(AgentSession::new()));

        // 1. 加载历史
        let history = session.get_messages().await?;

        // 2. 构造 system 消息（来自 Agent 构建时的 instructions / run_options）
        let effective_instructions = options.as_ref()
            .and_then(|o| o.instructions.as_deref())
            .unwrap_or(&self.instructions);
        let system_msg = ChatMessage::system(effective_instructions);

        // 3. 分离调用方传来的 messages — 通常只有 user 消息
        //    调用方无需传 system（已在 Agent 构建时设置）
        let user_messages: Vec<ChatMessage> = messages
            .into_iter()
            .filter(|m| m.role == MessageRole::User || m.role == MessageRole::Tool)
            .collect();

        // 4. 拼装完整消息列表
        //    合约: [system] + [history] + [user_message]
        //    system 在最前且内容不变 → 保证 KV cache 前缀稳定性
        //    history 包含所有历史 user + assistant + tool 消息
        let full_messages: Vec<ChatMessage> = std::iter::once(system_msg)
            .chain(history.into_iter())
            .chain(user_messages.into_iter())
            .collect();

        // 5. 记录本次请求哈希（用于后续缓存跟踪）
        session.touch_request_hash(&full_messages);

        // 6. 调用 inner agent
        let stream = self.inner.run(full_messages, Some(session.clone()), options).await;

        // 7. 流结束后将新消息写入 session
        // ... 通过 collect_agent_response 聚合后写入
        // session.add_message(user_msg).await;
        // session.add_message(assistant_msg).await;

        stream
    }
}

/// 调用方契约:
///   调用方只需传 vec![ChatMessage::user("问题")] — 无需传 system 或手动拼装 history
///   框架自动:
///     1. 从 Agent.instructions / AgentRunOptions.instructions 注入 system
///     2. 从 ISession 加载历史消息
///     3. 拼装为 [system] + history + [user_message]
///     4. 流结束后将 user + assistant 追加到 ISession
```

### 九、依赖说明

Session 模块需要 `chrono`（`DateTime<Utc>`）、`uuid`（`Uuid::new_v4()`）和 `serde_json`（序列化）。均为 minimal 依赖，不引入 Tower/Axum。

---

## What Changes

- **BREAKING**: `ContentMeta` → `ResponseMetadata`（重命名 + 新增 `agent_id`, `model_id`, `properties`）
- **BREAKING**: `AgentRunOptions` 新增 `properties: HashMap<String, Value>`
- **BREAKING**: `AgentBuilder::build() → Arc<dyn IAgent>`（统一门面）
- **BREAKING**: 新增 `AgentResponseConverter`（framework crate）— 架构核心转换器
- **BREAKING**: `ChatMessage` 新增 `tool_calls`, `tool_call_id`
- **BREAKING**: `ChatStreamChunk`, `AgentStreamChunk` → 替换为 `AgentResponseResult`(public) + `AgentResponseUpdate`(internal)
- **BREAKING**: 新增 `Content` enum (7 variants) + `Event` enum (3 variants)
- **BREAKING**: `IAgent` 新增 `get_subagent()`, `list_subagents()`
- `UsageStats` → `Usage` 提升到 core crate

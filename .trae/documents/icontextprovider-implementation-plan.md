# Rust Agent Framework — IContextProvider 上下文提供器实现计划

> 参考：Microsoft Agent Framework (MAF) `AIContextProvider` 抽象类设计模式
>
> 核心目标：为 Rust Agent Framework 构建可扩展的上下文工程基础设施，支持对话历史、RAG、Agent Skills、Wiki 等上下文增强能力以统一接口接入 Agent 管道。

---

## 一、MAF AIContextProvider 设计分析

### 1.1 MAF 核心架构

MAF 的 `AIContextProvider` 是一个**抽象基类**，作为 Agent 调用生命周期的扩展点：

```
Agent 调用
  │
  ├─ ① AIContextProvider.InvokingAsync()   ← Pre-invocation: 注入额外上下文
  │    返回 AIContext { Instructions, Messages, Tools }
  │
  ├─ ② LLM 推理调用（含 Tool Loop）
  │
  └─ ③ AIContextProvider.InvokedAsync()    ← Post-invocation: 提取/存储状态
      检查 request/response messages，更新 Provider 内部状态
```

**关键设计特性**：

| 特性 | MAF 做法 | Rust 适配策略 |
|------|---------|-------------|
| 抽象基类 + virtual 方法 | `abstract class AIContextProvider` | `#[async_trait] trait IContextProvider` |
| 多 Provider 链式执行 | `AIContextProviders = [...]` 按注册顺序执行 | `Vec<Arc<dyn IContextProvider>>` 顺序迭代 |
| Session 级隔离状态 | `ProviderSessionState<T>` 帮助类，状态存在 `AgentSession` 中 | `AgentSession.provider_states: HashMap<String, Value>` |
| 上下文注入载体 | `AIContext { Instructions, Messages, Tools }` | `ContextResult` struct |
| 两阶段生命周期 | `InvokingAsync` / `InvokedAsync` | `on_invoking()` / `on_invoked()` |
| Provider 实例共享 | 一个 Provider 实例附着到一个 Agent，跨所有 Session 共享 | 同设计：`Arc<dyn IContextProvider>` 存储在 Agent 上 |
| 序列化支持 | `SerializeAsync` / 反序列化构造函数 | 通过 `serde_json::Value` + `AgentSession.provider_states` 实现 |
| Provider 工厂模式 | `AIContextProviderFactory` 每次创建新实例 | 不使用工厂（Rust 中 Provider 实例由 Builder 构造直接传入） |

### 1.2 MAF ContextProvider vs Rust 框架现有 History Agent 规划

| | MAF `AIContextProvider` | Rust 当前规划 `HistoryAgent` |
|---|---|---|
| 定位 | 通用上下文注入框架，History 是其中一个实现 | 特定 Agent 装饰器，仅处理历史管理 |
| 注入内容 | instructions + messages + tools 三类 | 仅 messages（从 session 加载/持久化） |
| 后处理 | `InvokedAsync` 可提取状态、更新存储 | HistoryAgent 将响应持久化到 session |
| 可组合性 | 多个 Provider 链式执行 | 单一装饰器，不可链式组合 |

**结论**：`IContextProvider` 是 `HistoryAgent` 的超集。本计划用 `IContextProvider` 替代 `HistoryAgent`，`HistoryContextProvider` 成为 `IContextProvider` 的一个内置实现。

---

## 二、Rust 实现架构

### 2.1 管道架构

```
调用方 agent.run(messages, session, options)
  │
  ▼
┌──────────────────────────────────────────────────────────────────────┐
│  ContextProviderAgent  (新增 — 替代原规划的 HistoryAgent 层)          │
│                                                                      │
│  Phase 1: Pre-invocation — 链式执行所有 Provider.on_invoking()        │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │ [0] HistoryContextProvider   → 加载 Session 历史消息          │  │
│  │ [1] (future) SkillsProvider → 注入技能指令                    │  │
│  │ [2] (future) RAGProvider    → 检索知识库注入相关消息           │  │
│  │ [3] (future) WikiProvider   → 注入维基文档上下文              │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  Phase 2: 调用 inner Agent                                           │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │ ToolLoopAgent (如注册了 tools)                                 │  │
│  │   └── ChatClientAgent (纯 LLM 调用，不再包含 history 逻辑)     │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  Phase 3: Post-invocation — 链式执行所有 Provider.on_invoked()       │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │ [0] HistoryContextProvider   → 持久化新消息到 Session          │  │
│  │ [1] (future) MemoryProvider  → 提取长期记忆                    │  │
│  │ [2] (future) RAGProvider     → 更新 embedding 索引             │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
│  返回: BoxStream<AgentResponseResult>                                 │
└──────────────────────────────────────────────────────────────────────┘
```

### 2.2 组装方式

```rust
// 最终用户使用方式
let agent = AgentBuilder::new("assistant")
    .chat_client(deepseek_client)
    .instructions("你是一位乐于助人的助手")
    .with_tool(get_weather_tool)
    .with_context_provider(HistoryContextProvider::new())       // 内置
    // .with_context_provider(MySkillsProvider::new(skills_dir)) // 未来扩展
    // .with_context_provider(MyRagProvider::new(vector_store))  // 未来扩展
    .build()?;

let stream = agent.run(messages, session, options).await?;
```

AgentBuilder 内部组装顺序：
```
ContextProviderAgent(providers, 
  ToolLoopAgent(
    ChatClientAgent(chat_client, instructions, tools)
  )
)
```

---

## 三、详细变更清单

### 变更 1：定义 `IContextProvider` trait + `ContextResult`（core crate）

**文件**：`crates/core/src/context_provider.rs`（**新建**）

```rust
use async_trait::async_trait;
use std::sync::Arc;

use crate::{AgentRunOptions, AgentResponse, ChatMessage, IAgent, ISession, ITool, Result};

/// 上下文注入载体 — Provider 在 Pre-invocation 阶段返回的上下文增强内容
///
/// 对标 MAF 的 `AIContext` 返回类型，三种注入能力：
/// - instructions: 追加到 system prompt 的指令文本
/// - messages: 注入到消息列表的额外消息
/// - tools: 本次调用可用的动态工具
#[derive(Debug, Default)]
pub struct ContextResult {
    pub instructions: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<Arc<dyn ITool>>,
}

/// 上下文提供器 trait — Agent 调用生命周期的核心扩展点
///
/// 对标 MAF 的 `AIContextProvider` 抽象类：
/// - on_invoking()  → 对应 InvokingAsync  / ProvideAIContextAsync
/// - on_invoked()   → 对应 InvokedAsync   / StoreAIContextAsync
///
/// Provider 实例附着在一个 Agent 上，跨所有 Session 共享引用。
/// Session 级状态通过 `ISession` 的 provider_states 存储，而非保存在 Provider 实例上。
///
/// # 设计原则（Rust 惯用适配）
/// - 不使用继承 => 使用 `#[async_trait]` trait
/// - 不隐式传递 AgentRunContext => 通过方法参数显式传入 Agent/Session/Messages/Options
/// - 不依赖 DI 容器 => Builder 模式编译期注入 Provider 实例
#[async_trait]
pub trait IContextProvider: Send + Sync {
    /// Provider 唯一名称，用于 Session 状态 keying 和日志标识
    fn name(&self) -> &str;

    /// Pre-invocation 钩子：在 Agent 调用 LLM 之前执行
    ///
    /// Provider 可返回 `ContextResult` 来动态注入 instructions、messages、tools。
    /// 多个 Provider 的输出将被合并（instructions 追加，messages 拼入消息列表前端，
    /// tools 合并到可用工具集）。
    ///
    /// # 参数
    /// - agent: 当前 Agent 引用，可获取 id、metadata 等信息
    /// - session: 当前会话，可读写 provider_states
    /// - messages: 调用方传入的原始消息列表（Provider 可以检查以决定注入策略）
    /// - options: 本次调用的运行选项
    async fn on_invoking(
        &self,
        agent: &dyn IAgent,
        session: &dyn ISession,
        messages: &[ChatMessage],
        options: &AgentRunOptions,
    ) -> Result<ContextResult>;

    /// Post-invocation 钩子：在 Agent 收到 LLM 响应之后执行
    ///
    /// Provider 可以检查请求和响应消息，提取信息存入 session state。
    ///
    /// **注意**：此钩子在**流式响应完全收集后**调用，仅在单轮完整对话结束时触发。
    /// Tool Loop 的每轮迭代不触发此钩子。
    ///
    /// # 参数
    /// - agent: 当前 Agent 引用
    /// - session: 当前会话，可写 provider_states
    /// - request_messages: 发送给 LLM 的完整请求消息
    /// - response: 聚合后的 Agent 响应（文本 + tool_calls + usage）
    /// - error: 如果调用失败，携带错误信息；成功时为 None
    async fn on_invoked(
        &self,
        agent: &dyn IAgent,
        session: &dyn ISession,
        request_messages: &[ChatMessage],
        response: Option<&AgentResponse>,
        error: Option<&crate::AgentError>,
    ) -> Result<()>;
}
```

**决策说明**：
- trait 命名 `IContextProvider` 而不是 `ContextProvider`，与框架现有的 `IAgent`、`ISession`、`ITool`、`IChatClient` 风格一致
- `on_invoking()` 签名传递 &dyn IAgent 和 &dyn ISession，编译期通过 `dyn` 实现运行时多态
- `response` 参数是 `Option<&AgentResponse>`，成功时有值，失败时为 None（配合 error 判断）
- 不做 `run_id` / `Uuid` 之类的标识，因为这些属于 `AgentContext` 的职责（未来扩展点）

### 变更 2：ISession 新增 Provider State 支持（core crate）

**文件**：`crates/core/src/session.rs`（**修改**）

#### 2.1 新增类型 `ProviderStateStore`

```rust
/// Provider 级 Session 状态存储
///
/// 对标 MAF 的 `ProviderSessionState<T>` 机制。
/// 每个 Provider 以 `name()` 为 key 存储任意 JSON 值。
/// 同一 Provider 在所有 Session 间共享实例引用，但状态隔离在各自的 Session 中。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderStateStore {
    states: HashMap<String, serde_json::Value>,
}

impl ProviderStateStore {
    pub fn new() -> Self { Self { states: HashMap::new() } }

    pub fn get(&self, provider_name: &str) -> Option<&serde_json::Value> {
        self.states.get(provider_name)
    }

    pub fn get_or_default(&self, provider_name: &str) -> serde_json::Value {
        self.states.get(provider_name)
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    }

    pub fn set(&mut self, provider_name: &str, state: serde_json::Value) {
        self.states.insert(provider_name.to_string(), state);
    }

    pub fn remove(&mut self, provider_name: &str) {
        self.states.remove(provider_name);
    }
}
```

#### 2.2 ISession trait 扩展

在 `ISession` trait 中新增方法：

```rust
/// 获取指定 Provider 在本次 Session 中存储的状态
fn get_provider_state(&self, provider_name: &str) 
    -> Result<serde_json::Value>;

/// 设置指定 Provider 在本次 Session 中的状态
fn set_provider_state(&self, provider_name: &str, state: serde_json::Value) 
    -> Result<()>;
```

`AgentSession` 实现：
- 新增字段 `provider_states: RwLock<ProviderStateStore>`
- `get_provider_state()` → 读锁获取
- `set_provider_state()` → 写锁设置
- `Serializable` / `Deserialize` 中 `provider_states` 纳入序列化范围

#### 2.3 SessionSnapshot 扩展

```rust
pub struct SessionSnapshot {
    // ... 现有字段
    pub provider_states: ProviderStateStore,  // 新增
}
```

### 变更 3：实现 `HistoryContextProvider`（framework crate）

**文件**：`crates/framework/src/context_providers/history_provider.rs`（**新建**）

`HistoryContextProvider` 对标 MAF 的 `InMemoryHistoryProvider`：

```rust
use async_trait::async_trait;
use rust_agent_core::{
    ChatMessage, ContextResult, IAgent, IContextProvider,
    ISession, MessageRole, Result, AgentRunOptions, AgentResponse,
};

/// 对话历史上下文提供器
///
/// 对标 MAF 的 `InMemoryHistoryProvider`，职责：
/// - on_invoking: 从 Session 加载历史消息，注入到消息列表中
/// - on_invoked: 将本轮新消息持久化到 Session
pub struct HistoryContextProvider {
    /// 是否在 on_invoking 阶段加载历史消息（默认 true）
    load_messages: bool,
}

impl HistoryContextProvider {
    pub fn new() -> Self { Self { load_messages: true } }
    
    pub fn with_load_messages(mut self, load: bool) -> Self {
        self.load_messages = load;
        self
    }
}

impl Default for HistoryContextProvider {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl IContextProvider for HistoryContextProvider {
    fn name(&self) -> &str { "HistoryContextProvider" }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextResult> {
        let mut injection = ContextResult::default();
        
        if self.load_messages {
            let history = session.get_messages().await.unwrap_or_default();
            injection.messages = history;
        }
        
        Ok(injection)
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        session: &dyn ISession,
        request_messages: &[ChatMessage],
        response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        // 检查 session 中已有多少消息，找出新增的
        let existing_count = {
            let state = session.get_provider_state(self.name())
                .unwrap_or_default();
            state.get("last_message_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize
        };
        
        // request_messages 包含 [system + history + new_messages]
        // 只持久化本轮的 new_messages（排除 system 和 session 中已有的）
        let total = request_messages.len();
        let system_count = request_messages.iter()
            .filter(|m| m.role == MessageRole::System)
            .count();
        let new_start = system_count + existing_count;
        
        if new_start < total {
            for msg in &request_messages[new_start..] {
                if msg.role != MessageRole::System {
                    let _ = session.add_message(msg.clone()).await;
                }
            }
        }
        
        // 如果 response 中包含了 assistant 的文本/工具调用消息，也持久化
        if let Some(resp) = response {
            // 将 assistant 文本消息持久化
            if !resp.text.is_empty() {
                let _ = session.add_message(
                    ChatMessage::assistant(resp.text.clone())
                ).await;
            }
            // 将工具调用和结果持久化
            // （ToolLoopAgent 已在 ToolLoop 内部持久化了工具交互，
            //   这里不再重复。只持久化最终文本响应）
        }
        
        // 更新 provider state 中的消息计数
        let new_count = session.get_messages().await
            .map(|msgs| msgs.len())
            .unwrap_or(0);
        let _ = session.set_provider_state(
            self.name(),
            serde_json::json!({"last_message_count": new_count}),
        );
        
        Ok(())
    }
}
```

**设计决策**：
- `load_messages` 控制是否自动加载历史，默认 true
- 使用 `provider_state` 记录 `last_message_count` 避免重复持久化
- `on_invoked` 中持久化逻辑精细处理 system/history/new 的边界

### 变更 4：实现 `ContextProviderAgent` 管道编排（framework crate）

**文件**：`crates/framework/src/agents/context_provider_agent.rs`（**新建**）

```rust
use async_trait::async_trait;
use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions,
    BoxStream, ChatMessage, IAgent, IContextProvider, ISession,
    MessageRole, Result, collect_agent_response,
};
use futures_util::StreamExt;

/// ContextProviderAgent — IContextProvider 链的编排器
///
/// 对应 MAF 框架中 Agent 管道执行 ContextProvider 链的逻辑。
/// 替代原规划的 HistoryAgent 层，统一管理所有 IContextProvider 的执行。
///
/// 生命周期：
///   1. 顺序执行所有 providers.on_invoking()，合并 ContextResult
///   2. 组装完整消息列表：[新增instructions] + [provider消息] + [调用方消息]
///   3. 调用 inner agent
///   4. 流收集完成后顺序执行所有 providers.on_invoked()
///   5. 返回原始流（打入模式）或收集后重放
///
/// **注**：对于流式响应，Provider.invoked() 在流完全收集后执行。
/// 当前实现收集流然后重放，保证 on_invoked 先执行完再输出。
pub struct ContextProviderAgent {
    id: AgentId,
    metadata: AgentMetadata,
    inner: Arc<dyn IAgent>,
    providers: Vec<Arc<dyn IContextProvider>>,
}

impl ContextProviderAgent {
    pub fn new(
        name: impl Into<String>,
        inner: Arc<dyn IAgent>,
        providers: Vec<Arc<dyn IContextProvider>>,
    ) -> Self {
        let name = name.into();
        Self {
            id: AgentId::new(&name),
            metadata: AgentMetadata {
                agent_type: "ContextProviderAgent".to_string(),
                key: name.clone(),
                description: format!("Context providers wrapping {}", inner.id()),
            },
            inner,
            providers,
        }
    }
}

#[async_trait]
impl IAgent for ContextProviderAgent {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let run_options = options.unwrap_or_default();
        
        // Phase 1: Pre-invocation — 执行所有 Provider.on_invoking()
        let mut merged_instructions = String::new();
        let mut merged_provider_messages = Vec::new();
        let mut merged_tools = Vec::new();

        let session_ref = session.as_deref();
        
        for provider in &self.providers {
            let injection = provider.on_invoking(
                self.inner.as_ref(),
                session_ref.unwrap_or_else(|| unreachable!("session is required")),
                &messages,
                &run_options,
            ).await.unwrap_or_default();

            if let Some(inst) = injection.instructions {
                if !merged_instructions.is_empty() {
                    merged_instructions.push_str("\n\n");
                }
                merged_instructions.push_str(&inst);
            }
            merged_provider_messages.extend(injection.messages);
            merged_tools.extend(injection.tools);
        }

        // Phase 2: 组装完整消息列表
        // [system] + [provider_messages] + [caller_messages]
        let mut full_messages = Vec::new();
        
        // System 指令 = Agent instructions + Provider 注入的 instructions
        let effective_instructions = if let Some(ref override_inst) = run_options.instructions {
            override_inst.clone()
        } else {
            // Agent 级 instructions 通过 provider 注入，这里仅放 run_options 覆盖的
            String::new()
        };
        
        if !effective_instructions.is_empty() || !merged_instructions.is_empty() {
            let mut sys = effective_instructions;
            if !merged_instructions.is_empty() {
                if !sys.is_empty() { sys.push_str("\n\n"); }
                sys.push_str(&merged_instructions);
            }
            full_messages.push(ChatMessage::system(&sys));
        }
        
        // Provider 注入的消息（如 HistoryProvider 加载的历史记录）
        full_messages.extend(merged_provider_messages.into_iter()
            .filter(|m| m.role != MessageRole::System));
        
        // 调用方传入的消息
        full_messages.extend(messages.into_iter()
            .filter(|m| m.role != MessageRole::System));

        // 调用 inner agent
        let inner = Arc::clone(&self.inner);
        let session_clone = session.clone();
        let providers_clone = Arc::new(self.providers.clone());
        
        let stream = inner.run(full_messages.clone(), session.clone(), Some(run_options.clone())).await?;

        // Phase 3: Post-invocation — 收集流后执行 Provider.on_invoked()
        let stream = {
            let session = session_clone;
            let providers = providers_clone;
            let inner_ref = inner.clone();
            let request_messages = full_messages;

            async_stream::stream! {
                // 收集 inner stream 的所有 chunk
                // 这里采用 "打入" 模式：边收集中间响应边通过 channel 转发出
                // 流结束后执行 on_invoked，然后 emit 收集到的所有 chunk
                let (tx, rx) = tokio::sync::mpsc::channel::<Result<AgentResponseResult>>(256);
                let inner_clone = inner.clone();
                
                let collect = tokio::spawn(async move {
                    let mut stream = inner_clone.run(
                        request_messages.clone(),
                        session.clone(),
                        Some(run_options.clone()),
                    ).await?;
                    
                    let mut collected = Vec::new();
                    while let Some(chunk) = stream.next().await {
                        let _ = tx.send(chunk.clone()).await;
                        collected.push(chunk);
                    }
                    Ok::<_, rust_agent_core::AgentError>(collected)
                });

                // 边收集边转发
                let mut rx = rx;
                let mut all_chunks = Vec::new();
                while let Some(chunk) = rx.recv().await {
                    all_chunks.push(chunk);
                }
                
                // 等待收集完成
                if let Ok(Ok(collected)) = collect.await {
                    // 聚合响应
                    let response = collect_agent_response(
                        Box::pin(futures_util::stream::iter(
                            collected.iter().cloned()
                        ))
                    ).await.ok();
                    
                    // Phase 3: 执行 post-invocation
                    if let Some(ref sess) = session {
                        for provider in providers.iter() {
                            let _ = provider.on_invoked(
                                inner_ref.as_ref(),
                                sess.as_ref(),
                                &request_messages,
                                response.as_ref(),
                                None,
                            ).await;
                        }
                    }
                    
                    // 转发所有 chunk
                    for chunk in collected {
                        yield chunk;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.inner.get_subagent(id)
    }

    fn list_subagents(&self) -> Vec<Arc<dyn IAgent>> {
        self.inner.list_subagents()
    }

    async fn reset(&self) -> Result<()> {
        self.inner.reset().await
    }
}
```

**关键设计决策**：

1. **收集后执行 on_invoked**：流式响应需要完全收集才能执行 post-invocation。使用 "收集 + 重放" 模式。对于性能敏感场景，可改为 "异步通知" 模式（不阻塞第一个 chunk 的输出）。

2. **Provider 注入的 messages 排在 system 之后、caller messages 之前**：
   - 保证 history messages 在调用方消息之前（KV cache 前缀匹配）
   - 保证 RAG/Skills 注入的消息在用户消息可见范围内

3. **合并 Provider 注入的 instructions**：多个 Provider 的 instructions 以 `\n\n` 分隔拼接

### 变更 5：修改 `ChatClientAgent` — 移除内部 history 逻辑（framework crate）

**文件**：`crates/framework/src/chat_client_agent.rs`（**修改**）

变更内容：
- 删除 `ChatClientAgent.run()` 中的第 2-3 步（session history 加载和 message 持久化）
- `run()` 方法变为纯透传：接收 messages → 调用 IChatClient → 返回 stream
- Session 管理完全移交 `HistoryContextProvider`

**变更前**：
```rust
async fn run(&self, messages, session, options) {
    // 1. Determine effective instructions
    // 2. Build full messages: [system] + [session_history] + [caller messages]
    // 3. Persist caller messages to session (write-back)
    // 4. Build ChatClientRunOptions
    // 5. Call chat client
    // 6. Convert stream
}
```

**变更后**：
```rust
async fn run(&self, messages, session, options) {
    // 1. Determine effective instructions (仅 run_options override)
    // 2. Build full messages: [system(instructions)] + [caller messages]
    // 3. Build ChatClientRunOptions
    // 4. Call chat client
    // 5. Convert stream
}
```

### 变更 6：扩展 `AgentBuilder` 支持 ContextProvider（framework crate）

**文件**：`crates/framework/src/builder.rs`（**修改**）

新增方法：

```rust
pub struct AgentBuilder<C> {
    // ... 现有字段
    context_providers: Vec<Arc<dyn IContextProvider>>,  // 新增
}

impl<C: IChatClient + 'static> AgentBuilder<C> {
    // ... 现有方法

    /// 注册一个上下文提供器
    pub fn with_context_provider(
        mut self,
        provider: impl IContextProvider + 'static,
    ) -> Self {
        self.context_providers.push(Arc::new(provider));
        self
    }

    /// Build the agent stack:
    /// ContextProviderAgent(providers, ToolLoopAgent(ChatClientAgent)))
    pub fn build(self) -> Result<Arc<dyn IAgent>> {
        // 1. ChatClientAgent — 终端节点（不再含 history 逻辑）
        let chat_agent: Arc<dyn IAgent> = /* ... */;
        
        // 2. ToolLoopAgent — 如果注册了 tools
        let agent: Arc<dyn IAgent> = if !self.tools.is_empty() {
            Arc::new(ToolLoopAgent::new(/* ... */))
        } else {
            chat_agent
        };

        // 3. ContextProviderAgent — 如果有 context providers
        //    （替代原规划的 HistoryAgent 层）
        let agent: Arc<dyn IAgent> = if !self.context_providers.is_empty() {
            Arc::new(ContextProviderAgent::new(
                format!("{}-ctx-providers", self.agent_id),
                agent,
                self.context_providers,
            ))
        } else {
            agent
        };

        Ok(agent)
    }
}
```

### 变更 7：更新核心导出（core crate）

**文件**：`crates/core/src/lib.rs`（**修改**）

```rust
pub mod context_provider;  // 新增

pub use context_provider::{IContextProvider, ContextResult};  // 新增
```

### 变更 8：更新框架导出（framework crate）

**文件**：`crates/framework/src/lib.rs`（**修改**）

```rust
pub mod context_providers;  // 新增
pub mod agents;

pub use agents::context_provider_agent::ContextProviderAgent;  // 新增
pub use context_providers::history_provider::HistoryContextProvider;  // 新增
```

**文件**：`crates/framework/src/agents/mod.rs`（**修改**）

```rust
pub mod context_provider_agent;  // 新增
pub mod tool_loop_agent;
```

**文件**：`crates/framework/src/context_providers/mod.rs`（**新建**）

```rust
pub mod history_provider;
```

### 变更 9：`SessionSnapshot` / `serialize` / `deserialize` 适配

**文件**：`crates/core/src/session.rs`（**修改**）

- `AgentSession` 结构体中新增 `provider_states: RwLock<ProviderStateStore>` 
- `SessionSnapshot` 中新增 `provider_states: ProviderStateStore`
- `new()` / `with_id()` 初始化 `ProviderStateStore::new()`
- `snapshot()` 序列化 `provider_states`
- `serialize()` / `deserialize()` 包含 `provider_states`
- `ISession` trait 新增 `get_provider_state()` 和 `set_provider_state()` 方法

---

## 四、文件变更汇总

| 操作 | 文件 | 说明 |
|------|------|------|
| **新建** | `crates/core/src/context_provider.rs` | `IContextProvider` trait + `ContextResult` |
| **新建** | `crates/framework/src/agents/context_provider_agent.rs` | `ContextProviderAgent` 管道编排器 |
| **新建** | `crates/framework/src/context_providers/mod.rs` | Provider 模块入口 |
| **新建** | `crates/framework/src/context_providers/history_provider.rs` | `HistoryContextProvider` 实现 |
| **修改** | `crates/core/src/session.rs` | ISession 新增 `provider_states` 支持 |
| **修改** | `crates/core/src/lib.rs` | 导出 `IContextProvider`, `ContextResult` |
| **修改** | `crates/framework/src/lib.rs` | 导出 `ContextProviderAgent`, `HistoryContextProvider` |
| **修改** | `crates/framework/src/agents/mod.rs` | 新增 `context_provider_agent` 模块 |
| **修改** | `crates/framework/src/builder.rs` | `with_context_provider()` 方法 + 组装逻辑 |
| **修改** | `crates/framework/src/chat_client_agent.rs` | 移除 history 管理逻辑 |

---

## 五、兼容性与过渡策略

### 5.1 现有 CLI 代码

CLI 当前使用 `AgentBuilder` 构建 agent，不依赖内部的 history 逻辑细节。变更后：
- `AgentBuilder.build()` 行为变化：如果未注册任何 IContextProvider，管道中**没有** ContextProviderAgent 层，消息直接传给 inner agent（ChatClientAgent 只做 system instruction 拼接）
- 为向后兼容，可选：`AgentBuilder` 在 `build()` 时如果 `context_providers` 为空，**不自动**添加 `HistoryContextProvider`（保持最小意外原则）
- 也可以考虑当 `session.is_some()` 时自动添加，但 Phase 1 暂不做此行为

### 5.2 IAgent trait

`IAgent` trait 签名不变，`ContextProviderAgent` 是 `IAgent` 的透明装饰器。

### 5.3 ISession trait

新增的 `get_provider_state` / `set_provider_state` 方法带默认实现返回空/Noop，不强制破坏现有 ISession 实现者。

---

## 六、验证方案

### 6.1 单元测试

| 测试 | 文件 | 内容 |
|------|------|------|
| `ContextResult::default()` | `context_provider.rs` | 验证默认空注入 |
| `ProviderStateStore` CRUD | `session.rs` | 验证 get/set/remove/get_or_default |
| `HistoryContextProvider` 单轮 | `history_provider.rs` | 验证 on_invoking 加载空历史 |
| `HistoryContextProvider` 多轮 | `history_provider.rs` | 验证消息正确追加和持久化 |
| `ContextProviderAgent` — 空 providers | `context_provider_agent.rs` | 验证透传行为 |
| `ContextProviderAgent` — 单 provider | `context_provider_agent.rs` | 验证 on_invoking/on_invoked 执行顺序 |

### 6.2 集成测试

- **单轮对话**：`AgentBuilder.build()` → `agent.run([user("hello")], session)` → 验证响应正常
- **多轮对话**：`agent.run(turn1)` → `agent.run(turn2)` → 验证 Session 中消息正确积累
- **工具调用**：`agent.run([user("weather")], session)` → 验证 ToolLoop 正常工作且 HistoryProvider 持久化 tool interactions
- **Session 序列化**：`session.serialize()` → `AgentSession::deserialize()` → 验证 `provider_states` 恢复
- **Provider 链**：注册 2 个 mock providers → 验证 on_invoking 按注册顺序执行

---

## 七、未来扩展路线

本基础设施设计支撑以下未来扩展：

| 扩展 | 实现方式 | 预估文件 |
|------|---------|---------|
| `SkillsContextProvider` | 实现 IContextProvider，注入技能指令 + tools | `crates/framework/src/context_providers/skills_provider.rs` |
| `RagContextProvider` | 实现 IContextProvider，向量检索注入相关文档消息 | `crates/framework/src/context_providers/rag_provider.rs` |
| `MemoryContextProvider` | 实现 IContextProvider，LLM 提取长期记忆存储 | `crates/framework/src/context_providers/memory_provider.rs` |
| `TokenBudgetProvider` | 实现 IContextProvider，计算并限制上下文窗口 | `crates/framework/src/context_providers/token_budget_provider.rs` |
| `UserProfileProvider` | 实现 IContextProvider，注入用户偏好/配置 | `crates/framework/src/context_providers/user_profile_provider.rs` |

每种扩展只需：
1. 创建一个实现 `IContextProvider` trait 的 struct
2. 在 `AgentBuilder` 中通过 `.with_context_provider(MyProvider::new())` 注册
3. Framework 自动处理管道编排、合并、状态存储

---

## 八、风险与缓解

| 风险 | 缓解 |
|------|------|
| `ContextProviderAgent` 收集流再重放引入延迟 | Phase 1 采用收集+重放确保正确性；Phase 2 可优化为异步通知模式（on_invoked 不阻塞第一个 chunk） |
| `on_invoked` 执行失败阻塞流输出 | 每个 provider 的 `on_invoked` 用 `let _ =` 忽略错误（参考 ToolLoopAgent 做法），不阻塞主流 |
| Provider 执行顺序敏感 | 文档明确说明 Providers 按注册顺序执行，HistoryContextProvider 建议排在首位 |
| 向后兼容破坏 | `ISession` 新方法带默认实现；`AgentBuilder` 不自动注册 Provider；ChatClientAgent 保持独立可用 |

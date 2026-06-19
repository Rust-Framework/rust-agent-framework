# ChatClientAgent

`ChatClientAgent` 是 `IAgent` trait 的核心实现，对齐 MAF 的 `ChatClientAgent` 类。它是一个自包含的 Agent 运行时，持有 LLM 客户端、工具注册表、上下文提供器链和压缩策略。

## 结构体字段

```rust
pub struct ChatClientAgent {
    id: AgentId,                                       // Agent 唯一标识
    metadata: AgentMetadata,                           // 静态元数据
    chat_client: Arc<dyn IChatClient>,                 // LLM 客户端管道
    instructions: String,                              // 系统指令
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,     // 工具注册表（读写锁）
    context_providers: Vec<Arc<dyn IContextProvider>>,  // Provider 链
    compression_strategy: Option<Arc<dyn ICompressionStrategy>>,  // 压缩策略
    token_counter: Option<Arc<dyn ITokenCounter>>,     // Token 计数器
}
```

### 字段详解

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | `AgentId` | 构造时从 `name` 参数生成。 |
| `metadata` | `AgentMetadata` | `agent_type` 固定为 `"ChatClientAgent"`；`key` 同 `id`；`description`、`tool_names` 等通过方法设置。 |
| `chat_client` | `Arc<dyn IChatClient>` | 可以是管道包装后的客户端（`FunctionInvokingChatClient` → `DeepSeekChatClient`）。 |
| `instructions` | `String` | 系统指令文本。在 `run()` 中被组装为 `system` 消息。 |
| `tools` | `Arc<RwLock<ToolRegistry>>` | 使用 `RwLock` 支持并发读取工具列表。`run()` 中每次 LLM 调用前读取以生成工具定义。 |
| `context_providers` | `Vec<Arc<dyn IContextProvider>>` | 按注册顺序执行的 Provider 链。`AgentBuilder` 默认注入 `InMemoryHistoryProvider`。 |
| `compression_strategy` | `Option<Arc<dyn ICompressionStrategy>>` | 压缩策略（可选）。需要同时配置 `token_counter` 才生效。 |
| `token_counter` | `Option<Arc<dyn ITokenCounter>>` | Token 计数（可选）。配合压缩策略使用。 |

## 构造方法

### `new()`

```rust
pub fn new(name: impl Into<String>, chat_client: Arc<dyn IChatClient>) -> Self
```

创建最小化 Agent，仅设置 `id`、`metadata` 和 `chat_client`，其余字段使用默认值。

```rust
let client = Arc::new(DeepSeekChatClient::from_key("sk-...", "deepseek-chat")?);
let agent = ChatClientAgent::new("my-agent", client);
```

### 链式配置方法

```rust
// 设置指令
agent = agent.with_instructions("You are a helpful assistant.");

// 设置工具注册表
let mut registry = ToolRegistry::new();
registry.register(ReadFile { scope: None });
agent = agent.with_tools(registry);

// 设置描述
agent = agent.with_description("文件管理助手");

// 设置 ContextProvider 链（覆盖原有链）
agent = agent.with_context_providers(vec![
    Arc::new(InMemoryHistoryProvider::new()),
    Arc::new(WorkspaceContextProvider::new(scope)),
]);

// 设置压缩策略
agent = agent.with_compression_strategy(Arc::new(
    TokenBudgetStrategy::new()
));

// 设置 Token 计数器
agent = agent.with_token_counter(Arc::new(EstimateCounter::new()));
```

### 运行时访问

```rust
// 并发读取工具注册表
let tools_guard = agent.tools().await;
for tool in tools_guard.list() {
    println!("Tool: {}", tool.name());
}
// tools_guard 自动释放
```

## IAgent 实现

```rust
#[async_trait]
impl IAgent for ChatClientAgent {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        // 完整的三阶段生命周期（详见 run-lifecycle.md）
    }

    async fn reset(&self) -> Result<()> { Ok(()) }

    fn chat_client(&self) -> Option<&Arc<dyn IChatClient>> {
        Some(&self.chat_client)
    }
}
```

### `chat_client()` 暴露

`ChatClientAgent` 重写 `chat_client()` 返回 `Some(&self.chat_client)`。这使得 ContextProvider 能够通过 `agent.chat_client()` 发现底层 LLM 客户端。例如，`SkillMemoryContextProvider` 使用它创建 `MemoryAgent` 进行后台记忆整合。

### AgentProxy 辅助类型

在 Phase 3（Post-invocation）中，`ChatClientAgent` 创建一个 `AgentProxy` 实例传递给 Provider：

```rust
struct AgentProxy {
    id: AgentId,
    metadata: AgentMetadata,
    chat_client: Arc<dyn IChatClient>,
}
```

`AgentProxy` 的 `run()` 方法固定返回 `ConfigError`——它仅用于 `on_invoked()` 回调中提供 Agent 身份信息，不允许 Provider 通过它触发新的 LLM 调用。

## ChatClientAgent 与 AgentBuilder 的关系

`AgentBuilder` 是 `ChatClientAgent` 的构造器门面（Facade），处理了以下复杂性：

1. **默认 Provider**：自动注入 `InMemoryHistoryProvider`
2. **管道组装**：当注册工具时，用 `FunctionInvokingChatClient` 包裹叶子客户端
3. **ToolRegistry 初始化**：自动创建并注册工具的 `ToolRegistry`
4. **参数校验**：确保 `chat_client` 已设置

```rust
// AgentBuilder::build() 内部逻辑（简化版）
pub fn build(self) -> Result<Arc<dyn IAgent>> {
    let chat_client = self.chat_client
        .ok_or_else(|| AgentError::ConfigError("chat_client is required".into()))?;

    // 管道组装：如果有工具，包裹 FunctionInvokingChatClient
    let pipeline_client = if !self.tools.is_empty() {
        ChatClientBuilder::new()
            .leaf(Arc::new(chat_client))
            .use_decorator(Box::new(move |inner| {
                Arc::new(FunctionInvokingChatClient::new(inner, tools).with_max_rounds(max_rounds))
            }))
            .build()?
    } else {
        Arc::new(chat_client)
    };

    // 构造 ChatClientAgent 并链式配置
    let mut agent = ChatClientAgent::new(&self.agent_id, pipeline_client)
        .with_instructions(&self.instructions)
        .with_context_providers(self.context_providers);

    if let Some(strategy) = self.compression_strategy {
        agent = agent.with_compression_strategy(strategy);
    }
    if let Some(counter) = self.token_counter {
        agent = agent.with_token_counter(counter);
    }

    // 返回 trait object
    Ok(Arc::new(agent))
}
```

## 内部模块引用

`ChatClientAgent` 使用以下内部模块：

| 模块 | 用途 |
|------|------|
| `converter::AgentResponseConverter` | SSE 增量 → 结构化 Content/Event |
| `memory::memory_context::build_turn_transcript` | 构建本轮对话完整记录 |

### 方法提取：`spawn_post_invocation_handler`

`run()` 方法原本约 330 行，Phase 3（Post-invocation）的 channel 分叉逻辑已提取为独立方法 `spawn_post_invocation_handler`（位于 `impl ChatClientAgent` 块中）：

```rust
impl ChatClientAgent {
    fn spawn_post_invocation_handler(
        &self,
        converted: impl Stream<Item = Result<AgentResponseResult>> + Send + 'static,
        session: Option<Arc<dyn ISession>>,
        request_messages: Vec<ChatMessage>,
        caller_messages: Vec<ChatMessage>,
    ) -> impl Stream<Item = Result<AgentResponseResult>> + Send + 'static
}
```

该方法负责：
1. 通过 `tokio::sync::mpsc::unbounded_channel` 分叉流式响应
2. 后台收集完整响应后调用所有 `IContextProvider::on_invoked` 钩子
3. 将 assistant/tool 消息持久化到 session

提取后 `run()` 方法缩减至约 150 行，职责更清晰：Phase 1（预调用）→ Phase 1.5（压缩）→ Phase 2（LLM 调用）→ Phase 3（委托给 `spawn_post_invocation_handler`）。

## 下一步

理解 `ChatClientAgent` 的结构后，请阅读 **[AgentBuilder 构建器](./agent-builder.md)** 了解流畅构建器模式的完整使用指南。

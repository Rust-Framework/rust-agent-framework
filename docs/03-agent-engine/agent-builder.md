# AgentBuilder 构建器

`AgentBuilder` 是 RAF 最常用的构造模式——通过流畅的链式调用配置并构建一个完整配置的 Agent。它封装了 `ChatClientAgent` 的复杂构造逻辑（管道组装、默认 Provider 注入、参数校验）。

## 结构体定义

```rust
pub struct AgentBuilder<C> {
    agent_id: String,                                          // Agent ID
    chat_client: Option<C>,                                    // LLM 客户端
    instructions: String,                                      // 系统指令
    tools: Vec<Arc<dyn ITool>>,                                // 工具列表
    context_providers: Vec<Arc<dyn IContextProvider>>,         // Provider 链
    properties: HashMap<String, serde_json::Value>,            // 扩展属性
    description: String,                                       // 描述信息
    max_tool_rounds: usize,                                    // 最大工具轮数（默认 10）
    compression_strategy: Option<Arc<dyn ICompressionStrategy>>,  // 压缩策略
    token_counter: Option<Arc<dyn ITokenCounter>>,             // Token 计数器
}
```

## 完整使用示例

```rust
use std::sync::Arc;
use rust_agent_core::{WorkspaceScope, ScopePolicy, TokenBudgetStrategy, EstimateCounter};
use rust_agent_client::DeepSeekChatClient;
use rust_agent_framework::{
    AgentBuilder,
    tools::{ReadFile, WriteFile, ListFiles, RunCommand},
    context_providers::WorkspaceContextProvider,
};

#[tokio::main]
async fn main() -> rust_agent_core::Result<()> {
    // 1. 创建 LLM 客户端
    let client = DeepSeekChatClient::from_key("sk-...", "deepseek-chat")?;

    // 2. 配置工作区
    let scope = Arc::new(WorkspaceScope::new("/project", "my-project")
        .with_policy(ScopePolicy::ApproveOutside));

    // 3. 创建上下文提供器
    let workspace = WorkspaceContextProvider::new(Arc::clone(&scope))
        .add_tool(ReadFile { scope: None })
        .add_tool(WriteFile { scope: None })
        .add_tool(ListFiles { scope: None })
        .add_tool(RunCommand { scope: None, timeout_secs: None });

    // 4. 构建 Agent
    let agent = AgentBuilder::new("workspace-agent")
        .chat_client(client)                          // 必需
        .instructions("你是一个项目助手，能够读写文件和执行命令。") // 系统指令
        .with_description("工作区管理助手")              // 描述
        .add_context_provider(workspace)               // 添加 Provider（在默认 HistoryProvider 之后）
        .max_tool_rounds(8)                            // 工具调用循环上限
        .with_compression_strategy(                    // 压缩策略
            Arc::new(TokenBudgetStrategy::new().with_eviction_threshold(0.6))
        )
        .with_token_counter(                           // Token 计数器（压缩必需）
            Arc::new(EstimateCounter::new())
        )
        .with_properties([                             // 扩展属性
            ("environment".into(), serde_json::json!("production")),
        ])
        .build()?;                                     // 构建，返回 Arc<dyn IAgent>

    Ok(())
}
```

## 方法详解

### `new(id: impl Into<String>) -> Self`

创建构建器实例，自动初始化：

- `context_providers`：包含默认的 `InMemoryHistoryProvider`
- `max_tool_rounds`：默认 10
- 其他字段使用默认值

### `chat_client(client: C) -> Self`

**必须调用**。设置 LLM 客户端。`C` 必须实现 `IChatClient + 'static`。

```rust
AgentBuilder::new("agent")
    .chat_client(DeepSeekChatClient::from_key("sk-...", "deepseek-chat")?)
//  .chat_client(OpenAiChatClient::from_key(base_url, "sk-...", "gpt-4o")?)
    .build()?;
```

如果忘记调用，`build()` 返回 `Err(ConfigError("chat_client is required"))`。

### `instructions(text: impl Into<String>) -> Self`

设置系统指令文本。在 `run()` 时被组装为 `system` 角色的消息。

```rust
.instructions("你是一个专业的代码审查助手。你的职责包括：\n1. 检查代码风格\n2. 发现潜在 bug\n3. 提出改进建议")
```

也可以通过 `AgentRunOptions::with_instructions()` 在单次运行时覆盖：

```rust
let stream = agent.run(
    messages,
    Some(session),
    Some(AgentRunOptions::new().with_instructions("本次对话使用简洁风格回复")),
).await?;
```

### `with_tool(tool: impl ITool + 'static) -> Self`

注册单个工具。可多次调用注册多个工具。

```rust
.with_tool(ReadFile { scope: None })
.with_tool(WriteFile { scope: None })
.with_tool(RunCommand { scope: None, timeout_secs: None })
```

当注册了工具时，`build()` 自动用 `FunctionInvokingChatClient` 包裹 `IChatClient`，实现工具调用循环。

### `add_context_provider(provider: impl IContextProvider + 'static) -> Self`

追加一个 ContextProvider 到链中。Provider 按注册顺序依次执行。

```rust
// 链：InMemoryHistoryProvider → WorkspaceContextProvider → SkillsProvider
.add_context_provider(workspace_provider)
.add_context_provider(skills_provider)
```

### `add_context_provider_shared(provider: Arc<dyn IContextProvider>) -> Self`

添加一个共享的 Provider（`Arc`），便于在 Agent 外部保留引用。

```rust
let history = Arc::new(InMemoryHistoryProvider::new());
let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .add_context_provider_shared(Arc::clone(&history))
    .build()?;
// history 可以在 Agent 外部使用
```

### `with_history_provider(provider: impl IContextProvider + 'static) -> Self`

替换内置的 `InMemoryHistoryProvider` 为自定义实现。

```rust
.with_history_provider(RedisHistoryProvider::new(redis_client))
```

实现逻辑：在 Provider 链中定位 `InMemoryHistoryProvider`（按名称）并替换。如果不存在，则追加到链尾。

### `max_tool_rounds(rounds: usize) -> Self`

设置工具调用循环的最大轮次。当 LLM 连续调用工具超过此时限，`FunctionInvokingChatClient` 返回 `FinishReason::MaxRounds` 并终止循环。

```rust
.max_tool_rounds(3)  // 最多 3 轮工具调用后强制终止
```

默认值为 10。

### `with_description(desc: impl Into<String>) -> Self`

设置 Agent 的描述信息。存储在 `AgentMetadata.description` 中，供前端和发现机制使用。

```rust
.with_description("代码审查助手 — 检查代码风格、潜在 bug 和性能问题")
```

### `with_properties(iter) -> Self`

设置扩展属性键值对。存储在 `AgentMetadata` 中（通过 `ResponseMetadata.properties` 传递）。

```rust
.with_properties([
    ("team".into(), json!("backend")),
    ("version".into(), json!("1.0.0")),
])
```

### `with_compression_strategy(strategy: Arc<dyn ICompressionStrategy>) -> Self`

设置上下文窗口压缩策略。需要同时配置 `token_counter` 才生效。

```rust
.with_compression_strategy(Arc::new(SlidingWindowStrategy::new(50)))
.with_compression_strategy(Arc::new(TokenBudgetStrategy::new()))
.with_compression_strategy(Arc::new(
    CompressionPipeline::new()
        .add_strategy(Box::new(SlidingWindowStrategy::new(100)))
        .add_strategy(Box::new(TokenBudgetStrategy::new()))
))
```

### `with_token_counter(counter: Arc<dyn ITokenCounter>) -> Self`

设置 Token 计数器。配合压缩策略使用。内置实现：

```rust
// 估算计数器（无需额外依赖）
.with_token_counter(Arc::new(EstimateCounter::new()))

// 精确计数器（需启用 tiktoken feature）
#[cfg(feature = "tiktoken")]
.with_token_counter(Arc::new(TiktokenCounter::for_model("gpt-4")))
```

### `build() -> Result<Arc<dyn IAgent>>`

构建最终的 Agent，执行以下操作：

1. 校验 `chat_client` 已设置
2. 如果有工具，创建 `FunctionInvokingChatClient` 管道
3. 创建 `ChatClientAgent` 并配置所有参数
4. 返回 `Arc<dyn IAgent>` —— trait object，类型擦除

```rust
let agent: Arc<dyn IAgent> = AgentBuilder::new("my-agent")
    .chat_client(client)
    .build()?;
```

## ContextProvider 链的默认行为

### 默认 Provider

```rust
pub fn new(id: impl Into<String>) -> Self {
    Self {
        // ...
        context_providers: vec![
            Arc::new(InMemoryHistoryProvider::new()) as Arc<dyn IContextProvider>
        ],
        // ...
    }
}
```

`InMemoryHistoryProvider` 自动将 Session 中的历史消息注入到消息列表，实现多轮对话。

### Provider 链执行顺序

Provider 按 `Vec` 中的索引顺序执行：

```
[0] InMemoryHistoryProvider   ← 注入历史消息
[1] WorkspaceContextProvider  ← 注入工作区指令和工具
[2] AgentSkillsProvider       ← 注入 Skills 指令
[3] CompressionProvider       ← 压缩消息列表（replace_messages = true）
```

后续 Provider 可设置 `ContextInjection.replace_messages = true` 来**替换**之前累积的消息——这天然支持压缩策略。

## 构建器泛型参数

`AgentBuilder<C>` 的泛型参数 `C` 由 `chat_client()` 方法推断：

```rust
// C = DeepSeekChatClient
let builder: AgentBuilder<DeepSeekChatClient> = AgentBuilder::new("agent")
    .chat_client(deepseek_client);

// C = OpenAiChatClient
let builder: AgentBuilder<OpenAiChatClient> = AgentBuilder::new("agent")
    .chat_client(openai_client);
```

`build()` 要求 `C: IChatClient + 'static`。

## 错误处理

| 错误场景 | 返回 |
|----------|------|
| 未设置 `chat_client` | `Err(ConfigError("chat_client is required"))` |
| `ChatClientBuilder::build()` 失败 | 管道组装错误 |

## 下一步

掌握 `AgentBuilder` 后，继续阅读 **[Run 生命周期](./run-lifecycle.md)**，理解 `run()` 方法的三阶段内部机制。

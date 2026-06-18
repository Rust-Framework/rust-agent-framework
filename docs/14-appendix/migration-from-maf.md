# 14.5 从 MAF 迁移指南

本文档提供从 Microsoft Agent Framework (MAF) 到 Rust Agent Framework (RAF) 的迁移参考，涵盖概念映射、API 差异和最佳实践。

## 概念映射

### 核心概念对照

| MAF 概念 | RAF 对应 | 说明 |
|----------|---------|------|
| `ChatClientAgent` | `ChatClientAgent` | 相同的类名和职责，基于 IChatClient 的 Agent 实现 |
| `AIAgent` | `IAgent` | Agent 核心接口 |
| `AIFunction` | `ITool` | 工具接口 |
| `ApprovalRequiredAIFunction` | `ApprovalRequiredTool` | 需要审批的工具包装 |
| `AIContext` / `AIContextProvider` | `ContextResult` / `IContextProvider` | 上下文注入 |
| `AgentThread` / `ChatHistory` | `ISession` / `AgentSession` | 会话管理 |
| `ChatClient` | `IChatClient` | LLM 客户端 |
| `FunctionInvokingChatClient` | `FunctionInvokingChatClient` | 工具调用循环装饰器 |
| `ToolCallResult` | `ToolResult` | 工具执行结果 |
| `AgentResponse` | `AgentResponseResult` | Agent 响应 |
| `SequentialWorkflow` | `SequentialWorkflow` | 顺序编排 |
| `Workflow` / `WorkflowGraph` | `WorkflowGraph` | 工作流图 |
| `AgentSchema` | `AgentSchema v1.0` | 声明式配置规范 |
| `AgentDocument` | `AgentDocument` | 声明式文档类型 |

### 类型系统差异

| MAF (C#/TypeScript) | RAF (Rust) | 说明 |
|---------------------|------------|------|
| `Task<AgentResponse>` | `BoxStream<'static, Result<AgentResponseResult>>` | 异步流式返回 |
| `IEnumerable<T>` | `Vec<T>` 或 `BoxStream` | 集合和流 |
| `CancellationToken` | `Arc<AtomicBool>` | 取消令牌 |
| `event EventHandler<T>` | `UnboundedSender<T>` | 事件通道 |
| `Dictionary<string, object>` | `HashMap<String, Value>` | 字典/元数据 |

## 工具注册差异

### MAF（C#）

```csharp
// MAF: 通过特性注册
[Description("读取文件内容")]
public class ReadFileTool
{
    [Function("read_file")]
    public async Task<string> ReadFileAsync(
        [Description("文件路径")] string path)
    {
        return await File.ReadAllTextAsync(path);
    }
}

agent.AddTool<ReadFileTool>();
```

### RAF（Rust）

```rust
// RAF: 通过 #[tool] 宏注册
#[tool(description = "读取文件内容")]
async fn read_file(
    #[param(desc = "文件路径")] path: String,
) -> rust_agent_core::ToolResult {
    let content = tokio::fs::read_to_string(&path).await?;
    rust_agent_core::ToolResult::success(serde_json::json!({"content": content}))
}

// 注册
let agent = AgentBuilder::new("agent")
    .with_tool(ReadFile)
    .build()?;

// 或声明式（JSON）
{
    "kind": "function",
    "name": "read_file",
    "description": "读取文件内容"
}
```

## Provider 模式差异

### MAF Provider 模式

```csharp
// MAF: 使用工厂模式
var client = new OpenAIChatClient(model, apiKey);

// 或使用 DI 容器
services.AddSingleton<IChatClient>(new OpenAIChatClient(model, apiKey));
```

### RAF Provider 模式

```rust
// RAF: 使用构建器模式
use rust_agent_client::{DeepSeekChatClient, ChatClientOptions};

let client = DeepSeekChatClient::new(
    ChatClientOptions::deepseek("deepseek-v4-flash", api_key)
)?;

// 或 OpenAI 兼容
let client = DeepSeekChatClient::new(
    ChatClientOptions::openai("gpt-4o", api_key)
)?;
```

## 异步/流式模型差异

### MAF（C#）

```csharp
// MAF: IAsyncEnumerable 流式
await foreach (var update in agent.RunAsync(messages))
{
    Console.Write(update.Content);
}
```

### RAF（Rust）

```rust
// RAF: Futures Stream
use futures_util::StreamExt;

let mut stream = agent.run(messages, session, options).await?;
while let Some(chunk) = stream.next().await {
    match chunk {
        Ok(result) => {
            for content in &result.contents {
                if let Content::Text(ref t) = content {
                    print!("{}", t.delta);
                }
            }
        }
        Err(e) => eprintln!("错误: {}", e),
    }
}
```

## 会话管理差异

| 特性 | MAF | RAF |
|------|-----|-----|
| 会话创建 | `agent.NewThread()` | `agent.create_session()` |
| 历史管理 | `ChatHistory` | `ISession::messages()` |
| 状态持久化 | `IAgentState` | `ProviderState`（通过 ISession） |
| 会话隔离 | AgentThread 级别 | SessionBridge + AgentSession |

## 编排差异

### MAF

```csharp
// MAF: SequentialWorkflow
var workflow = new SequentialWorkflow(agent1, agent2, agent3);
```

### RAF

```rust
// RAF: SequentialWorkflow (builder pattern)
let workflow = SequentialWorkflow::new()
    .add_agent(agent1)
    .add_agent(agent2)
    .add_agent(agent3);

// 或直接构造
let workflow = SequentialWorkflow::from_agents(vec![agent1, agent2, agent3]);

// 包装为 IAgent
let agent = workflow.as_agent();
```

## AgentSchema 兼容性

RAF 的 AgentSchema v1.0 与 MAF 完全兼容：

```json
{
    "kind": "prompt",
    "name": "my-agent",
    "model": {
        "id": "deepseek-v4-flash",
        "connection": {
            "kind": "key",
            "api_key": "$DEEPSEEK_API_KEY"
        }
    },
    "instructions": "You are a helpful assistant.",
    "tools": [
        {"kind": "function", "name": "read_file", "description": "读取文件"}
    ]
}
```

上述 JSON 可以同时被 MAF 和 RAF 解析。

## 迁移清单

| 步骤 | 说明 |
|------|------|
| 1. 工具迁移 | 将 MAF 工具类转换为 `#[tool]` 宏标注的异步函数 |
| 2. Provider 切换 | 从 MAF 的 IChatClient 切换到 RAF 的 DeepSeekChatClient |
| 3. 流式处理 | 从 `IAsyncEnumerable` 切换到 `StreamExt` |
| 4. 会话管理 | 从 `AgentThread` 切换到 `AgentSession` |
| 5. 编排模式 | 使用 `SequentialWorkflow`、`ConcurrentWorkflow` 等对应类型 |
| 6. 配置文件 | 使用相同的 AgentSchema v1.0 格式（无需修改） |
| 7. 部署方式 | 从 .NET Host 切换到 `rust-agent-host` |

## 主要差异总结

| 方面 | MAF | RAF |
|------|-----|-----|
| **语言** | C# / TypeScript | Rust |
| **异步模型** | async/await + IAsyncEnumerable | async/await + Stream |
| **工具定义** | 特性 + 反射 | 过程宏（编译期） |
| **类型安全** | 运行时检查 | 编译期检查 |
| **内存管理** | GC | 所有权 + 借用 |
| **性能** | 托管运行时 | 零成本抽象 + 无 GC |
| **配置** | AgentSchema v1.0 | AgentSchema v1.0（兼容） |

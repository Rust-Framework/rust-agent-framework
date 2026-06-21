# 第一个智能体

本文通过一个完整示例，引导你使用 `AgentBuilder` 构建第一个 RAF 智能体——从初始化 LLM 客户端，到注册工具，再到流式输出处理。

## 整体架构

```mermaid
flowchart LR
    User[用户消息] --> Agent[ChatClientAgent]
    Agent --> CP[ContextProvider 链]
    CP --> Client[DeepSeekChatClient]
    Client --> LLM[LLM API]
    LLM --> Stream[流式响应]
    Stream --> Conv[AgentResponseConverter]
    Conv --> Output[AgentResponseResult]
    Output --> User
```

## 完整示例：一个带工具的智能体

```rust
use futures_util::StreamExt;
use rust_agent_core::{
    ChatMessage, Content, ITool, ToolResult,
};
use rust_agent_client::DeepSeekChatClient;
use rust_agent_framework::AgentBuilder;
use rust_agent_macros::tool;

/// 定义一个简单的回显工具
#[tool(description = "Echoes back the input text.")]
struct Echo;

impl Echo {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            text: String,
        }
        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| rust_agent_core::AgentError::ToolError(e.to_string()))?;
        Ok(ToolResult::success(serde_json::json!({"echo": args.text})))
    }
}

#[tokio::main]
async fn main() -> rust_agent_core::Result<()> {
    // 1. 创建 LLM 客户端
    let client = DeepSeekChatClient::from_key("sk-...", "deepseek-chat")?;

    // 2. 使用 AgentBuilder 构建智能体
    let agent = AgentBuilder::new("my-agent")
        .chat_client(client)
        .instructions("你是一个友好的助手，可以使用 echo 工具回显用户输入。")
        .with_tool(Echo)
        .max_tool_rounds(5)  // 最多 5 轮工具调用循环
        .build()?;

    // 3. 创建会话
    let session = agent.create_session();

    // 4. 发送消息并处理流式响应
    let messages = vec![ChatMessage::user("请用 echo 工具帮我复述：你好，世界！")];
    let mut stream = agent.run(messages, Some(session), None).await?;

    let mut full_text = String::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(chunk) => {
                for content in chunk.contents {
                    match content {
                        Content::Text(t) => {
                            print!("{}", t.delta);
                            full_text.push_str(&t.delta);
                        }
                        Content::ToolCalling(c) => {
                            println!("\n[调用工具] {} 参数: {}", c.name, c.arguments);
                        }
                        Content::ToolCalled(c) => {
                            println!("[工具结果] {}", c.result.unwrap_or_default());
                        }
                        Content::Finish(f) => {
                            println!("\n[完成] 原因: {:?}", f.finish_reason);
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => eprintln!("错误: {}", e),
        }
    }

    Ok(())
}
```

## 关键步骤解析

### 1. 创建 LLM 客户端

```rust
let client = DeepSeekChatClient::from_key("sk-...", "deepseek-chat")?;
```

`DeepSeekChatClient` 通过静态方法 `from_key()` 创建，内部构造 `ChatClientOptions` 并初始化 HTTP 客户端。也支持 OpenAI 兼容 API：

```rust
let client = OpenAiChatClient::from_key("https://api.openai.com/v1", "sk-...", "gpt-4o")?;
```

### 2. AgentBuilder 构建器模式

```rust
let agent = AgentBuilder::new("my-agent")        // 设置 Agent ID
    .chat_client(client)                          // 注入 LLM 客户端
    .instructions("...")                          // 设置系统指令
    .with_tool(Echo)                              // 注册工具
    .max_tool_rounds(5)                           // 工具调用循环上限
    .build()?;                                    // 构建，返回 Result
```

`AgentBuilder::build()` 返回 `Arc<dyn IAgent>` ——与具体类型解耦，方便替换和测试。

### 3. 会话管理

```rust
let session = agent.create_session();
```

`create_session()` 返回 `Arc<dyn ISession>`，默认实现为 `AgentSession`（内存存储），自动生成 UUID 作为会话 ID。会话维护消息历史，传递给 `run()` 方法后框架自动追加助手消息和工具执行结果。

你也可以手动创建会话：

```rust
use rust_agent_core::AgentSession;
let session = Arc::new(AgentSession::new());
```

### 4. 流式响应处理

`agent.run()` 返回 `BoxStream<'static, Result<AgentResponseResult>>`。每次 `yield` 产生一个 `AgentResponseResult` 块，包含 `contents: Vec<Content>` 和事件信息。

`Content` 枚举有 12 个变体，流式处理只需关注常用的：

| Content 变体 | 含义 | 何时出现 |
|-------------|------|----------|
| `Text(delta)` | 文本增量 | LLM 流式输出文本 |
| `Reasoning(delta)` | 推理文本增量 | DeepSeek thinking 模式 |
| `ToolCallStart` | 工具调用开始 | LLM 决定调用工具 |
| `ToolCallArgs(args_delta)` | 工具参数片段 | 流式输出工具参数 |
| `ToolCalling(name, args)` | 完整工具调用 | 参数流结束，汇总为结构化 JSON |
| `ToolCalled(result/error)` | 工具执行结果 | 工具执行完成 |
| `Usage(usage)` | 用量统计 | 每次响应结束时 |

### 5. 流式文本收集（简化版）

如果不需要逐字处理，可使用 `collect_agent_response` 一次性收集：

```rust
use rust_agent_core::collect_agent_response;

let stream = agent.run(messages, Some(session), None).await?;
let response = collect_agent_response(stream).await?;
println!("完整回复: {}", response.text);
println!("工具调用: {:?}", response.tool_calls);
println!("用量: {:?}", response.usage);
```

## #[tool] 宏：两种定义方式

### 方式一：结构体 + call 方法（推荐）

```rust
#[tool(description = "Adds two numbers.")]
struct Add;

impl Add {
    async fn call(&self, arguments: serde_json::Value) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args { a: f64, b: f64 }
        let args: Args = serde_json::from_value(arguments)
            .map_err(|e| rust_agent_core::AgentError::ToolError(e.to_string()))?;
        Ok(ToolResult::success(serde_json::json!({"result": args.a + args.b})))
    }
}
```

宏自动生成 `ITool` trait 实现，从 `description` 属性生成 `description()` 返回值，从 `call` 方法参数结构体自动推导 JSON Schema。

### 方式二：异步函数（函数式风格）

```rust
#[tool(description = "Converts Celsius to Fahrenheit.")]
async fn celsius_to_fahrenheit(
    #[param(desc = "Temperature in Celsius")] celsius: f64,
) -> rust_agent_core::ToolResult {
    let fahrenheit = celsius * 9.0 / 5.0 + 32.0;
    ToolResult::success(serde_json::json!({"fahrenheit": fahrenheit}))
}
```

宏生成帕斯卡命名（`CelsiusToFahrenheit`）的 `ITool` 实现。

## 输出示例

运行上述代码，你将看到类似输出：

```
[调用工具] echo 参数: {"text":"你好，世界！"}
[工具结果] {"echo":"你好，世界！"}
我已经使用 echo 工具回显了你的输入："你好，世界！"
[完成] 原因: Stop
```

## 下一步

理解了构建智能体的基本流程后，请阅读 **[核心概念](./core-concepts.md)** 深入理解框架的架构设计。

---

## 声明式构建（附）：一行 YAML 替代手写代码

从 v0.1.0 起，RAF 支持通过 YAML 声明文件驱动 Agent 构建，避免将系统指令和工具配置硬编码在 Rust 源码中。

### 声明文件: `cli-agent.yaml`

```yaml
kind: prompt
name: my-agent
model:
  id: agnes-2.0-flash
  provider: deepseek
  connection:
    kind: key
    api_key: $DEEPSEEK_API_KEY          # 从环境变量读取，部署时无需改代码
instructions: |
  You are a helpful AI assistant.
  Respond concisely in the user's language.

tools:
  - kind: web                    # 无 name → 注册全部 web 工具
  - kind: function
    name: echo
    description: Echoes back the input text

contexts:                         # 声明式上下文提供器
  - kind: memory
    name: skill-memory
    config:
      directory: logs/memory
      consolidationInterval: 1

max_tool_rounds: 8
```

### 声明式加载

```rust
use rust_agent_decl::DeclAgentBuilder;
use std::sync::Arc;

// 模型、API Key、contexts、tools 全部从 YAML 读取——无需代码硬编码
let agent = DeclAgentBuilder::new()
    .from_yaml_file("cli-agent.yaml")
    .with_tool("echo", |_| Ok(Arc::new(Echo)))  // 注册 YAML 中声明的自定义工具
    .build()
    .await?;

// 运行时模型切换（保留 escape hatch）
let agent_with_model = DeclAgentBuilder::new()
    .from_yaml_file("cli-agent.yaml")
    .with_model("deepseek-reasoner")             // 覆盖 YAML 中的模型
    .with_tool("echo", |_| Ok(Arc::new(Echo)))
    .build()
    .await?;
```

### 两种方式的对比

| 维度 | AgentBuilder（手写） | DeclAgentBuilder（声明式） |
|------|---------------------|-------------------------|
| 构建入口 | `AgentBuilder::new("name")` | `DeclAgentBuilder::new().from_yaml_file(path)` |
| 系统指令 | `.instructions("...")` 硬编码字符串 | YAML 文件，修改无需重编译 |
| 模型配置 | `.chat_client(client)` | `.with_model("gpt-4o")` |
| 工具注入 | `.with_tool(Echo)` | `.with_tool("echo", factory)` |
| 上下文注入 | `.add_context_provider_shared(cp)` | YAML `contexts` 段（memory/skills/mcp/workspace/knowledge/wiki）+ `.with_context(cp)` 外挂 |
| 产物 | `Arc<dyn IAgent>` | `Arc<dyn IAgent>` |

### 何时使用哪种方式

- **AgentBuilder**：快速原型、临时实验、工具数量少且指令简单的场景
- **DeclAgentBuilder**：生产部署、指令需要频繁迭代、团队协作维护 Agent 配置的场景
- 两种方式可以互相替代，因为它们产出的都是 `Arc<dyn IAgent>`，与框架其余部分完全兼容

### 交互式体验：ReplRunner

`ReplRunner` 是开箱即用的 REPL 运行器，提供最小化即可运行的编码体验：

```rust
use rust_agent_cli::ReplRunner;

// 最小方式：一行启动
ReplRunner::new(agent).run().await?;

// 完整方式：带模型切换和重启能力
ReplRunner::new(agent)
    .prompt("🦀 > ")
    .banner("My AI Chat — Type /help for commands")
    .on_switch_model(move |model| {
        Box::pin(async move {
            DeclAgentBuilder::new()
                .from_yaml_file("cli-agent.yaml")
                .with_model(&model)
                .build().await.map_err(Into::into)
        })
    })
    .on_restart(move || {
        Box::pin(async move {
            DeclAgentBuilder::new()
                .from_yaml_file("cli-agent.yaml")
                .build().await.map_err(Into::into)
        })
    })
    .run()
    .await?;
```

内置命令：`/help` `/clear` `/think on/off` `/model <name>` `/restart` `/quit`。


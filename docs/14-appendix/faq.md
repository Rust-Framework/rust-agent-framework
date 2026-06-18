# 14.4 常见问题（FAQ）

## 基础使用

### Q: 如何创建我的第一个 Agent？

```rust
use rust_agent_framework::AgentBuilder;
use rust_agent_client::DeepSeekChatClient;

let client = DeepSeekChatClient::new(
    ChatClientOptions::deepseek("deepseek-v4-flash", "your-api-key")
)?;

let agent = AgentBuilder::new("my_agent")
    .chat_client(client)
    .instructions("你是一个有用的助手。")
    .build()?;

let messages = vec![ChatMessage::user("你好！")];
let stream = agent.run(messages, None, None).await?;
```

### Q: 如何添加自定义工具？

通过 `#[tool]` 宏创建自定义工具：

```rust
#[tool(description = "我的自定义工具")]
async fn my_tool(
    #[param(desc = "参数描述")] param: String,
) -> rust_agent_core::ToolResult {
    // 业务逻辑
    rust_agent_core::ToolResult::success(serde_json::json!({"result": "ok"}))
}

// 注册到 Agent
let agent = AgentBuilder::new("my_agent")
    .chat_client(client)
    .with_tool(MyTool)
    .build()?;
```

### Q: 如何添加自定义 LLM 提供商？

实现 `IChatClient` trait：

```rust
struct MyChatClient { /* ... */ }

#[async_trait]
impl IChatClient for MyChatClient {
    fn model_id(&self) -> &str { "my-model" }
    fn model_metadata(&self) -> Option<&ModelMetadata> { None }

    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        // 实现与你的 LLM API 的通信
        // 返回流式 AgentResponseUpdate
    }
}
```

### Q: `#[tool]` 宏可以用于哪些类型？

`#[tool]` 支持：
- **异步函数**（`async fn`）：自动生成 ITool 实现、参数 Schema 和反序列化器
- **结构体**（`struct`）：需要手动实现 `call(&self, arguments: Value) -> Result<ToolResult>`，宏生成委托的 ITool 实现

不支持普通函数、trait 方法或其他项目类型。

## 工具系统

### Q: 如何处理工具执行错误？

```rust
#[tool(description = "示例工具")]
async fn my_tool(path: String) -> rust_agent_core::ToolResult {
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            rust_agent_core::ToolResult::success(serde_json::json!({"content": content}))
        }
        Err(e) => {
            // 工具级错误 — 使用 ToolResult::error
            rust_agent_core::ToolResult::error(format!("文件读取失败: {}", e))
        }
    }
}
```

`ToolResult::error` 返回的错误会反馈给 LLM，使其可以尝试修正参数重试。

### Q: 如何实现需要人工审批的工具？

```rust
use rust_agent_core::ApprovalRequiredTool;

let safe_command = RunCommand::default();
let dangerous_tool = ApprovalRequiredTool::new(Arc::new(safe_command));

let agent = AgentBuilder::new("safe_agent")
    .chat_client(client)
    .with_tool(dangerous_tool) // 执行前需要人工审批
    .build()?;
```

### Q: 如何限制工具的调用次数？

通过 `AgentBuilder::max_tool_rounds()` 设置：

```rust
let agent = AgentBuilder::new("limited_agent")
    .chat_client(client)
    .max_tool_rounds(10) // 最多 10 轮工具调用
    .build()?;
```

## 会话管理

### Q: 生产环境应该使用什么会话存储？

目前 RAF 内置的实现包括：
- `AgentSession`（内存存储）：适合开发环境
- `FileSessionStore`（文件存储）：适合单机部署
- `IsolatedSessionStore`（隔离存储）：适合多租户场景

对于生产环境，建议实现 `ISessionStore` trait 对接外部存储（如 PostgreSQL、Redis 等）。

### Q: 如何实现跨会话的记忆持久化？

使用 `SkillMemoryContextProvider`：

```rust
let memory = SkillMemoryContextProvider::new("./agent_memory")
    .with_consolidation_interval(5); // 每 5 轮触发记忆整合

let agent = AgentBuilder::new("memorized_agent")
    .chat_client(client)
    .with_context_provider(memory)
    .build()?;
```

记忆会持久化到 `./agent_memory/` 目录。

### Q: 会话之间如何共享状态？

通过 `ISession::set_provider_state()` 和 `get_provider_state()`：

```rust
// 在 ContextProvider 中保存状态
session.set_provider_state("my_key", json!({"data": "value"}))?;

// 在后续调用中读取
let state = session.get_provider_state("my_key")?;
```

## 多 Agent 编排

### Q: 如何实现 Agent 链式调用？

使用 `SequentialWorkflow`：

```rust
let workflow = SequentialWorkflow::new()
    .add_agent(agent_1)
    .add_agent(agent_2)
    .add_agent(agent_3);

let stream = workflow.run(input, session, options).await?;
```

### Q: 如何让多个 Agent 并行工作？

使用 `ConcurrentWorkflow`：

```rust
let workflow = ConcurrentWorkflow::from_agents(vec![agent_1, agent_2, agent_3]);
let stream = workflow.run(input, session, options).await?;
```

### Q: 如何实现智能路由（根据内容分发给不同 Agent）？

使用 `HandoffWorkflow`：

```rust
let workflow = HandoffWorkflow::new()
    .triage(router_agent) // 分类 Agent
    .agent(coding_agent)   // 目标 Agent
    .agent(writing_agent)
    .agent(analysis_agent)
    .build()?;
```

### Q: 如何构建自定义工作流图？

使用 `WorkflowBuilder`：

```rust
let mut builder = WorkflowBuilder::new("custom_flow");
builder.add_node("step1", executor_1, "第一步");
builder.add_node("step2", executor_2, "第二步");
builder.add_edge("step1", vec!["step2"]);
builder.mark_output("step2");
let graph = builder.build()?;
```

### Q: 如何扩展到大量 Agent？

1. 使用 WebSocket 传输模式部署为独立服务
2. 通过 `AgentRegistry` 组织 Agent 层次结构
3. 利用 `get_subagent()` 实现树形 Agent 发现
4. 为不同客户端创建独立的 ACP 会话
5. 使用 `SessionBridge` 隔离状态

## 扩展能力

### Q: 如何让 Agent 搜索互联网？

```rust
use rust_agent_websearch::WebSearch;

let agent = AgentBuilder::new("researcher")
    .chat_client(client)
    .with_tool(WebSearch)
    .build()?;
```

无需 API Key（使用 DuckDuckGo 后端）。

### Q: 如何实现 RAG（检索增强生成）？

实现 RAG trait 体系中的组件并通过 ContextProvider 集成：

```rust
// 1. 实现 IRetriever
// 2. 创建 ContextProvider 在调用前自动检索
// 3. 或创建 ITool 让 Agent 主动调用
```

### Q: 如何使用 Rhai 脚本扩展 Agent？

作为工具：

```rust
let tool = RhaiTool::new("calculator", "计算工具",
    json!({"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}}),
    "args.x + args.y",
);

let agent = AgentBuilder::new("scriptable").chat_client(client).with_tool(tool).build()?;
```

### Q: 如何添加可复用的 Agent 技能？

1. 创建 SKILL.md 文件
2. 使用 `AgentSkill::from_dir()` 加载
3. 通过 `AgentSkillsProvider` 注入 Agent

## 部署

### Q: 如何部署 RAF Agent 为服务？

```bash
# WebSocket 模式
rust-agent-host --mode ws --bind 0.0.0.0:9876 --api-key $DEEPSEEK_API_KEY

# 或 Stdio 模式（IDE 集成）
rust-agent-host --mode stdio --api-key $DEEPSEEK_API_KEY
```

### Q: 如何整合多个 Agent 到一个实例？

```toml
# host.toml
[agents]
coding = true
general = true
analysis = true

# 或指定声明式 Agent 目录
agents_dir = "./my_agents"
```

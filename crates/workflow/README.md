# rust-agent-workflow

Multi-agent orchestration layer — graph-based engine, built-in orchestration
patterns, and IAgent facade for seamless sub-agent discovery. Aligned with
[Microsoft Agent Framework (MAF)](https://learn.microsoft.com/zh-cn/agent-framework/workflows/orchestrations/) orchestration model.

## 快速开始

```rust
use rust_agent_workflow::orchestrations::{SequentialWorkflow, HandoffWorkflow, ConcurrentWorkflow};

// 1) 构建智能体
let researcher = AgentBuilder::new("researcher")
    .chat_client(client)
    .instructions("你是研究员，列出 3 个要点。")
    .build()?;

let summarizer = AgentBuilder::new("summarizer")
    .chat_client(client)
    .instructions("你是总结专家，一句话总结。")
    .build()?;

// 2) 顺序工作流
let workflow = SequentialWorkflow::from_agents(vec![researcher, summarizer]);
let stream = workflow.run(input_messages, session, options).await?;

// 3) 当不需要流式编排，可以直接 as_agent() 获得 IAgent
let agent: Arc<dyn IAgent> = workflow.as_agent();
let stream = agent.run(messages, session, options).await?;
```

---

## 内置编排 (`orchestrations`)

对齐 MAF `agent_framework.orchestrations`，《已内置大模型集成测试验证》](https://gitcode.com/rf2026/rust-agent-framework/)。

### SequentialWorkflow — 顺序编排

按顺序执行代理，前一个代理的输出作为下一个代理的输入。

```rust
use rust_agent_workflow::orchestrations::SequentialWorkflow;

let workflow = SequentialWorkflow::new()
    .add_agent(researcher)
    .add_agent(summarizer);

// 或直接传入 agents 列表
let workflow = SequentialWorkflow::from_agents(vec![researcher, summarizer]);

let stream = workflow.run(messages, session, options).await?;
```

**执行模型**：

```
[user input] → Agent 1 (收集输出) → Agent 2 (收集输出) → ... → Agent N (流式)
```

**最佳实践**：
- 适合管道式处理：调研 → 写作 → 审查
- 中间步骤的输出被收集为完整文本再传入下一步
- 最后一步保留流式输出，前端可实时展示

### ConcurrentWorkflow — 并发编排

所有代理并行执行相同输入，输出流合并交织。

```rust
use rust_agent_workflow::orchestrations::ConcurrentWorkflow;

let workflow = ConcurrentWorkflow::new()
    .add_agent(analyst_a)
    .add_agent(analyst_b);

// 或直接传入
let workflow = ConcurrentWorkflow::from_agents(vec![analyst_a, analyst_b]);

let merged = workflow.run(messages, session, options).await?;
```

**执行模型**：

```
[user input] ─┬─ Agent A (并行) ─┐
              └─ Agent B (并行) ─┴─→ 合并流 (select_all)
```

**最佳实践**：
- 适合多视角分析：技术角度 + 商业角度同时评估
- 输出是交织的，前端可按 `author_name` 区分来源
- 适合并行调用多个外部 API

### HandoffWorkflow — 交接路由

Triage 代理分析请求，自动路由到最匹配的专家代理。

```rust
use rust_agent_workflow::orchestrations::{HandoffWorkflow, HandoffBuilder};

let workflow = HandoffWorkflow::new()
    .triage(triage_agent)           // 路由决策代理
    .agent(code_expert)             // 代码专家 ← 通过 metadata.description 匹配
    .agent(writing_expert)          // 写作专家
    .build()?;

let stream = workflow.run(messages, session, options).await?;
```

**执行模型**：

```
[user input] → Triage (分析 → 选择专家)
                    ├─ "代码专家" → Code Expert (流式)
                    └─ "写作专家" → Writing Expert (流式)
```

**最佳实践**：
- 为每个 Agent 设置 `with_description("代码专家")` — triage 通过描述匹配目标
- Triage agent 的 instructions 应明确要求"只回复代理名称"
- 若 triage 响应未匹配任何代理名称，将直接返回 triage 输出（fallback）
- 初始化 Agent 数量建议 3-5 个，超出会影响 triage 精度

---

## as_agent() → IAgent 统一门面

MAF 核心设计哲学：`WorkflowBuilder.build() → Workflow.as_agent() → IAgent`。

所有编排模式均可通过 `as_agent()` 转换为统一的 `IAgent` 接口：

```rust
let agent: Arc<dyn IAgent> = workflow.as_agent();

// 统一 IAgent 接口
agent.id()                      // "handoff_triage_coder_writer"
agent.metadata()                // AgentMetadata { agent_type, description, ... }
agent.get_subagent(&agent_id)   // → Option<Arc<dyn IAgent>>
agent.run(messages, session).await?  // 流式输出
agent.reset().await?            // 递归重置所有子代理
```

### get_subagent() — 子代理发现

前端可通过 `get_subagent()` 获取子代理引用，实现：
- 代理树导航（展开/折叠子代理）
- 点击子代理查看其详细信息
- 独立调用子代理进行单独对话

```rust
let agent: Arc<dyn IAgent> = workflow.as_agent();

// 遍历子代理
let child = agent.get_subagent(&AgentId::new("code-expert"));
if let Some(child) = child {
    // 前端展示：[code-expert] 正在运行中…
    let child_stream = child.run(messages, session, options).await?;
}
```

---

## 自定义编排 (WorkflowBuilder + WorkflowEngine)

当内置编排模式无法满足需求时，使用底层图引擎构建自定义工作流。

### 声明式图构建

```rust
use rust_agent_workflow::{
    WorkflowBuilder, WorkflowEngine,
    FunctionExecutor, AgentExecutor,
};

// 构建带检查点的图工作流
let graph = WorkflowBuilder::new()
    .add_agent_node("researcher", researcher)   // ← IAgent 自动包装为 AgentExecutor
    .add_node("validator", Arc::new(FunctionExecutor::new(
        "validator",
        |msg: String| vec![format!("验证结果: {}", msg)]
    )))
    .set_start("researcher")
    .add_edge("researcher", "validator")         // 直接边
    .add_fan_out_edge("validator", vec!["writer_a", "writer_b"])  // 扇出
    .add_fan_in_edge(vec!["writer_a", "writer_b"], "aggregator")  // 扇入
    .with_output_from("aggregator")
    .build()?;

let engine = WorkflowEngine::new(graph)
    .with_checkpoint_manager(checkpoint_manager);  // 可选：启用检查点
```

### 边类型

| 边 | API | 行为 |
|----|-----|------|
| 直接边 | `.add_edge(src, dst)` | 消息 1:1 路由 |
| 扇出 | `.add_fan_out_edge(src, vec![dst1, dst2])` | 消息复制到所有目标 |
| 扇入 | `.add_fan_in_edge(vec![src1, src2], dst)` | 所有源到达后才触发目标 |

### 自定义 Executor

实现 `IExecutor` trait 创建自定义节点：

```rust
struct SummarizerExecutor;

#[async_trait]
impl IExecutor for SummarizerExecutor {
    fn id(&self) -> &str { "summarizer" }
    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new(std::any::type_name::<String>())]
    }

    async fn handle(
        &self,
        message: Box<dyn Any + Send + Sync>,
        _ctx: &dyn IWorkflowContext,
        _progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let input = message.downcast::<String>().unwrap();
        Ok(HandlerResult::Messages(vec![Box::new(format!("总结: {}", input))]))
    }
}
```

---

## Checkpoint 集成

WorkflowEngine 支持检查点——在 SuperStep 完成后自动保存状态，支持故障恢复。

```rust
use rust_agent_workflow::{InMemoryCheckpointStore, CheckpointManager};

let store = Arc::new(InMemoryCheckpointStore::new());
let cp = Arc::new(CheckpointManager::with_default_config(store));

let engine = WorkflowEngine::new(graph)
    .with_checkpoint_manager(cp);

// 引擎将自动在每轮 SuperStep 后保存状态
let (events, outputs) = engine.run(initial_msg, session).await?;
```

**日志输出**（`RUST_LOG=debug`）：

```
DEBUG WorkflowEngine::execute_loop starting  node_count=3 has_checkpoint=true
DEBUG Checkpoint: create_initial              session_id=xxx
DEBUG SuperStep: entering step=0              active_nodes=researcher
DEBUG Node: completed                         node_id=researcher
DEBUG SuperStep: completed step=0             messages_routed=1
DEBUG Checkpoint: commit step=0               state_keys=0
 INFO WorkflowEngine::execute_loop completed  total_steps=3
```

---

## 生产环境最佳实践

### 1. 智能体构建

```rust
// ✅ 好：每个 Agent 独立 ChatClient（避免共享状态竞争）
let agent_a = AgentBuilder::new("a")
    .chat_client(DeepSeekChatClient::new(options.clone())?)
    .with_description("代码专家")    // ← 用于 HandoffWorkflow triage 匹配
    .build()?;

let agent_b = AgentBuilder::new("b")
    .chat_client(DeepSeekChatClient::new(options)?)  // 独立实例
    .with_description("写作专家")
    .build()?;
```

```rust
// ❌ 避免：克隆 Arc<dyn IChatClient> 会导致共享状态和协程交错
// 每个 Agent 应该有独立的 ChatClient 实例
```

### 2. Session 管理

```rust
// ✅ 共享 Session：跨步骤保留对话历史
let session = Arc::new(AgentSession::with_id("workflow-session"));
let stream = workflow.run(messages, Some(session.clone()), options).await?;

// ✅ 独立 Session：每个子代理拥有独立对话上下文
let sub_session = Arc::new(AgentSession::with_id("sub-agent-session"));
let sub_stream = sub_agent.run(messages, Some(sub_session), options).await?;
```

### 3. 错误处理

```rust
match workflow.run(messages, session, options).await {
    Ok(mut stream) => {
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(result) => { /* 处理正常输出 */ }
                Err(e) => tracing::error!(error = %e, "Stream chunk error"),
            }
        }
    }
    Err(e) => {
        tracing::error!(error = %e, "Workflow execution failed");
        // 检查点恢复
        if let Some(checkpoint) = checkpoint_manager.load_latest(&session_id).await? {
            // 从上一检查点重试
        }
    }
}
```

### 4. 资源管理

```rust
// ✅ 使用 as_agent() 统一生命周期管理
let agent = workflow.as_agent();
tokio::spawn(async move {
    // agent 被 move 进来，工作流完成后自动 drop
    let stream = agent.run(messages, session, options).await?;
}).await?;

// ✅ reset() 递归重置所有子代理
agent.reset().await?;
```

### 5. 流式输出消费

```rust
use futures_util::StreamExt;

let mut stream = Box::pin(workflow.run(messages, session, options).await?);
while let Some(chunk) = stream.next().await {
    match chunk {
        Ok(result) => {
            for content in &result.contents {
                match content {
                    Content::Text(t) => print!("{}", t.delta),
                    Content::ToolCallStart(inner) => {
                        tracing::info!(tool = %inner.name, "Tool call started");
                    }
                    Content::ToolCalled(inner) => {
                        if let Some(err) = &inner.error {
                            tracing::error!(error = %err, "Tool call failed");
                        }
                    }
                    _ => {}
                }
            }
        }
        Err(e) => tracing::error!(error = %e, "Stream error"),
    }
}
```

### 6. 检查点配置

```rust
// 使用 InMemoryCheckpointStore（开发/测试）
let store = Arc::new(InMemoryCheckpointStore::new());

// 使用 FileCheckpointStore（持久化）
let store = Arc::new(FileCheckpointStore::new("./checkpoints"));

// 配置增量提交策略
let config = CheckpointConfig {
    full_snapshot_interval: 10,  // 每 10 个 step 生成完整快照
    ..Default::default()
};
let cp = Arc::new(CheckpointManager::new(store, config));
```

### 7. 并发控制

```rust
// ConcurrentWorkflow 中的并发度等于 agents 数量
// 大量 Agent 并发时注意 API rate limit
let workflow = ConcurrentWorkflow::from_agents(vec![a1, a2, a3, a4, a5]);
// ← 5 个 Agent 将并行调用 LLM API，注意 rate limit 配置
```

---

## API 速查表

### 内置编排

| API | 说明 |
|-----|------|
| `SequentialWorkflow::new().add_agent(a).build()` | 顺序编排 |
| `SequentialWorkflow::from_agents(vec![...])` | 直接构造（对齐 MAF） |
| `ConcurrentWorkflow::new().add_agent(a)` | 并发编排 |
| `HandoffWorkflow::new().triage(t).agent(a).build()?` | 交接路由 |
| `.as_agent() → Arc<dyn IAgent>` | 转为统一门面 |
| `.run(input, session, options) → BoxStream` | 直接运行 |

### 图引擎

| API | 说明 |
|-----|------|
| `WorkflowBuilder::new().add_agent_node().set_start().build()` | 声明式图构建 |
| `WorkflowEngine::new(graph).with_checkpoint_manager(cp)` | 带检查点引擎 |
| `engine.run(initial_msg, session) → (events, outputs)` | 双通道执行 |
| `FunctionExecutor::new(id, handler)` | 纯函数节点 |
| `AgentExecutor::new(agent)` → `IExecutor` | Agent 节点包装 |

### 向后兼容别名

旧名称仍可用，无需迁移：

```rust
use rust_agent_workflow::orchestrations::{
    SequentialWorkflow as SequentialPattern,   // 旧名
    ConcurrentWorkflow as ConcurrentPattern,
    ConcurrentWorkflow as FanOutWorkflow,
    HandoffWorkflow as HandoffPattern,
};
```

---

## 依赖

- `rust-agent-core` — IAgent、ISession、AgentMetadata 等核心抽象
- `rust-agent-framework` — AgentBuilder、ChatClientAgent、FunctionInvokingChatClient
- `futures-core` / `futures-util` — 流操作（select_all、StreamExt）
- `tokio` — 异步运行时
- `async-trait` — 异步 trait 支持
- `serde` / `serde_json` — 序列化（检查点持久化）
- `tracing` — 结构化日志
- `chrono` — 时间处理
- `parking_lot` — 高性能锁
- `uuid` — 唯一 ID 生成

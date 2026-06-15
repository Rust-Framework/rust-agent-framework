# crates/workflow 架构设计计划

## 摘要

参照微软 MAF（Microsoft Agent Framework）Workflows 模块的设计原则，将当前的 `crates/workflow` 从简单的模式封装层升级为完整的**图驱动工作流引擎**。同时针对 MAF 的已知缺陷（子代理运行状况不可观测）进行架构级改进，通过全链路流式事件系统实现执行过程对前端透明可见。

核心设计遵循 MAF 六大支柱，并在此基础上增加 **第七支柱 — 全链路可观测性**：

1. 图拓扑定义
2. Builder 构建器
3. SuperStep 执行模型
4. 双阶段状态管理
5. 检查点/恢复
6. 类型安全路由
7. **全链路流式事件系统**（RAF 原创改进）

---

## 1. 当前状态分析

### 1.1 现有代码结构

```
crates/workflow/src/
├── lib.rs              # 公开导出
├── graph_flow.rs       # GraphFlow — MVP（仅有注册+入口执行）
└── patterns/
    ├── mod.rs
    ├── sequential.rs   # 顺序执行，管道传递
    ├── concurrent.rs   # 扇出/扇入，stream select_all 合并
    └── handoff.rs      # 占位实现（仅调用 triage agent）
```

### 1.2 现有问题

| 问题 | 说明 |
|------|------|
| **无图拓扑** | `GraphFlow` 只有 agents HashMap + entry_agent，没有边（Edge）概念 |
| **无执行引擎** | `run()` 直接委托给 entry agent，没有多步遍历、消息路由、SuperStep |
| **无状态管理** | 无法在节点间持久化/共享状态，无法跨步骤恢复 |
| **无检查点** | 运行不可暂停/恢复，不可序列化 |
| **无类型路由** | 消息仅以 `Vec<ChatMessage>` 传递，无类型安全分发 |
| **Patterns 独立** | 模式直接调用 `IAgent::run()`，未复用引擎 |
| **无生命周期钩子** | 无法在节点初始化、检查点前后、消息投递前后插入逻辑 |
| **不可观测** | 执行过程是黑盒 — 子代理何时开始、进度如何、何时完成、是否出错，外部完全不可知 |

### 1.3 已有基础

```text
workflow 
  ├── rust-agent-core     (IAgent, ISession, IChatClient, IContextProvider, ITool, BoxStream, 
  │                        AgentError, ChatMessage, AgentResponseResult, Content, Event, …)
  └── rust-agent-framework (AgentBuilder, AgentRuntime, ChatClientAgent, ToolLoopAgent)
```

---

## 2. MAF 设计原则映射 + RAF 改进

### 2.1 七大设计原则 → Rust 映射

| 原则 | C# 实现 | Rust 对应方案 |
|------|---------|-------------|
| **图拓扑定义** | `Workflow` 不可变图 | `WorkflowGraph`（HashMap + HashSet，`Arc` 共享） |
| **Builder 构建器** | `WorkflowBuilder` 流式 API | `WorkflowBuilder` 流式 API |
| **SuperStep 执行** | `InProcessRunner.RunSuperStepAsync()` | `WorkflowEngine::execute_super_step()` |
| **状态管理** | `StateManager` 两阶段写入 | `StateStore`（`RwLock` + 待提交缓冲区） |
| **检查点** | `Checkpoint` + `ICheckpointManager` | `Checkpoint` + `ICheckpointStore` trait |
| **类型安全路由** | 反射 + TypeId 字典 | `TypeTag` + proc macro |
| **全链路流式事件** | 仅末端 `IAsyncEnumerable<WorkflowEvent>` | **`BoxStream<WorkflowEvent>` 贯穿全链路** |

### 2.2 MAF 的问题：子代理不可观测

**MAF 的现象**：`WorkflowSession.InvokeStageAsync()` 虽然返回 `IAsyncEnumerable<AgentResponseUpdate>`，但消费者只能看到最终的文本/工具调用输出，无法知道：

- 当前正在执行哪个 Executor/Agent
- 该 Agent 是否正在思考、调用工具、还是已完成
- 多 Agent 并发时各自的进度
- 错误发生在哪个节点

**RAF 的改进方案**：引入 `WorkflowEvent` 枚举，覆盖执行全生命周期的关键节点，通过统一的 `BoxStream<WorkflowEvent>` 从引擎流出，前端可逐事件消费。

**事件粒度设计**：

```
WorkflowEvent 层次:
├── WorkflowStarted    — 工作流启动，携带 graph 元信息
├── StepLifecycle      — SuperStep 开始/完成
│   ├── SuperStepStarted
│   └── SuperStepCompleted
├── NodeLifecycle      — 节点激活/就绪/完成/失败
│   ├── NodeInvoking   — 节点即将执行
│   ├── NodeStreaming  — 节点流式产出（Agent 文本/工具调用增量）
│   ├── NodeCompleted  — 节点执行完成
│   └── NodeFailed     — 节点执行失败
├── Output             — 工作流输出
│   └── AgentResponse
└── WorkflowError / WorkflowCompleted
```

**对前端的意义**：

- 可绘制工作流 DAG 图，高亮当前活跃节点
- 可显示每个 Agent 的实时输出流（打字机效果）
- 可显示每个 Agent 的状态徽标（等待中 / 执行中 / 已完成 / 失败）
- 多 Agent 并发时展示并行进度条

### 2.3 接口命名规范

所有 trait / interface 定义统一使用 `I` 前缀，与现有代码库一致：

| 抽象 | 命名 |
|------|------|
| 执行器 trait | `IExecutor` |
| 边执行器 trait | `IEdgeRunner` |
| 工作流上下文 trait | `IWorkflowContext` |
| 检查点存储 trait | `ICheckpointStore` |
| 边条件 trait | `IEdgeCondition` |
| 扇出分配器 trait | `IFanOutAssigner` |
| 发言者选择器 trait | `ISpeakerSelector` |
| 终止条件 trait | `ITerminationCondition` |
| 类型标记 trait | `ITypeTagged` |
| 检查点管理器 trait | `ICheckpointManager` |

---

## 3. 目标架构全景

### 3.1 模块树

```
crates/workflow/src/
├── lib.rs                        # crate 根
│
├── graph/                        # 图定义
│   ├── mod.rs
│   ├── workflow_graph.rs         # WorkflowGraph
│   ├── edge.rs                   # Edge 枚举 (Direct, FanOut, FanIn)
│   ├── edge_data.rs              # 边数据载体
│   ├── node.rs                   # Node
│   └── port.rs                   # RequestPort
│
├── executor/                     # 执行器层
│   ├── mod.rs
│   ├── base.rs                   # IExecutor trait
│   ├── registry.rs               # ExecutorRegistry
│   ├── function_executor.rs      # FunctionExecutor<F>
│   └── agent_executor.rs         # AgentExecutor（IAgent → IExecutor 桥接）
│
├── builder/                      # 构建器
│   ├── mod.rs
│   └── workflow_builder.rs       # WorkflowBuilder
│
├── engine/                       # 执行引擎
│   ├── mod.rs
│   ├── engine.rs                 # WorkflowEngine
│   ├── edge_runner.rs            # IEdgeRunner + Direct/FanOut/FanIn 实现
│   ├── message_router.rs         # MessageRouter
│   ├── message_envelope.rs       # MessageEnvelope
│   ├── step_context.rs           # StepContext
│   ├── work_context.rs           # IWorkflowContext（Executor 的服务接口）
│   └── event.rs                  # WorkflowEvent 枚举 — 全链路事件系统
│
├── state/                        # 状态管理
│   ├── mod.rs
│   ├── state_store.rs            # StateStore
│   └── scope.rs                  # ScopeId / UpdateKey
│
├── checkpoint/                   # 检查点
│   ├── mod.rs
│   ├── checkpoint.rs             # Checkpoint
│   ├── store.rs                  # ICheckpointStore + InMemoryCheckpointStore
│   └── manager.rs                # CheckpointManager
│
├── patterns/                     # 编排模式（基于引擎重构）
│   ├── mod.rs
│   ├── sequential.rs
│   ├── concurrent.rs
│   ├── handoff.rs
│   └── group_chat.rs
│
└── macros/                       # 过程宏
    └── mod.rs
```

### 3.2 分层架构图 + 数据流

```
┌─────────────────────────────────────────────────────────────┐
│  patterns/  (Sequential, Concurrent, Handoff, GroupChat)     │  ← 专业化编排
├─────────────────────────────────────────────────────────────┤
│  builder/   (WorkflowBuilder)                                │  ← 声明式构建
├─────────────────────────────────────────────────────────────┤
│  engine/ ─── BoxStream<WorkflowEvent> ───► 前端消费          │  ← 事件流出口
│  ├─ WorkflowEngine  (SuperStep 循环)                        │
│  ├─ IEdgeRunner x3   (Direct / FanOut / FanIn)              │
│  ├─ StepContext      (单步消息队列)                          │
│  └─ IWorkflowContext (send_message / emit_event / state)    │
├─────────────────────────────────────────────────────────────┤
│  state/      (StateStore — 两阶段写入)                       │
│  checkpoint/ (ICheckpointStore — 持久化)                     │
├─────────────────────────────────────────────────────────────┤
│  graph/     (WorkflowGraph, Edge, Node, Port)                │
│  executor/  (IExecutor, AgentExecutor, FunctionExecutor)     │
├─────────────────────────────────────────────────────────────┤
│  rust-agent-core / rust-agent-framework                      │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. 详细模块设计

### 4.1 graph/ — 图定义层

#### `WorkflowGraph`

```rust
pub struct WorkflowGraph {
    pub(crate) nodes: HashMap<String, Arc<dyn IExecutor>>,
    pub(crate) edges: HashMap<String, HashSet<Edge>>,  
    pub(crate) ports: HashMap<String, RequestPort>,
    pub(crate) output_executors: HashSet<String>,
    pub(crate) start_node_id: String,
}
```

- `build()` 后冻结，无公开修改接口
- `validate()` → `Result<()>`：校验可达性、边引用完整性

#### `Edge` 枚举

```rust
pub enum Edge {
    Direct(DirectEdgeData),
    FanOut(FanOutEdgeData),
    FanIn(FanInEdgeData),
}

pub struct DirectEdgeData {
    pub edge_id:        EdgeId,
    pub source_id:      String,
    pub sink_id:        String,
    pub label:          Option<String>,
    pub condition:      Option<Box<dyn IEdgeCondition>>,
}

pub struct FanOutEdgeData {
    pub edge_id:        EdgeId,
    pub source_id:      String,
    pub sink_ids:       Vec<String>,
    pub label:          Option<String>,
    pub assigner:       Option<Box<dyn IFanOutAssigner>>,
}

pub struct FanInEdgeData {
    pub edge_id:        EdgeId,
    pub source_ids:     Vec<String>,
    pub sink_id:        String,
    pub label:          Option<String>,
}

#[async_trait]
pub trait IEdgeCondition: Send + Sync {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> Result<bool>;
}

#[async_trait]
pub trait IFanOutAssigner: Send + Sync {
    fn targets(&self, envelope: &MessageEnvelope) -> Vec<String>;
}
```

### 4.2 executor/ — 执行器层

#### `IExecutor` trait

```rust
#[async_trait]
pub trait IExecutor: Send + Sync {
    fn id(&self) -> &str;
    fn accepted_types(&self) -> Vec<TypeTag>;
    fn send_types(&self) -> Vec<TypeTag>;
    fn is_output(&self) -> bool { false }

    // ── 生命周期钩子 ──
    async fn on_init(&self, _ctx: &dyn IWorkflowContext) -> Result<()> { Ok(()) }
    async fn on_checkpoint_save(&self, _ctx: &dyn IWorkflowContext) -> Result<()> { Ok(()) }
    async fn on_checkpoint_restore(&self, _ctx: &dyn IWorkflowContext) -> Result<()> { Ok(()) }
    async fn on_delivery_start(&self, _ctx: &dyn IWorkflowContext) -> Result<()> { Ok(()) }
    async fn on_delivery_end(&self, _ctx: &dyn IWorkflowContext) -> Result<()> { Ok(()) }

    /// 核心执行方法。
    ///
    /// `progress` 是一个 mpsc Sender，执行器通过它发送增量进度事件。
    /// 引擎将 progress 事件包装为 `WorkflowEvent::NodeStreaming` 对外广播。
    /// 此设计保证了：
    /// 1. 执行器内部不需要知道 WorkflowEvent 的存在
    /// 2. 引擎统一控制事件包装和广播
    /// 3. 前端可以实时接收每个节点的流式输出
    async fn handle(
        &self,
        message: Box<dyn Any + Send>,
        ctx: &dyn IWorkflowContext,
        progress: tokio::sync::mpsc::UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult>;
}

pub enum HandlerResult {
    Messages(Vec<Box<dyn Any + Send>>),  // 沿边发送
    Output(Box<dyn Any + Send>),         // 直接输出
    None,
}

/// 节点进度事件 — 引擎收集后包装为 WorkflowEvent::NodeStreaming
#[derive(Clone)]
pub enum NodeProgress {
    /// 文本增量
    TextDelta(String),
    /// 推理文本增量
    ReasoningDelta(String),
    /// 工具调用开始
    ToolCallStart { call_id: String, name: String },
    /// 工具调用参数增量
    ToolCallArgs { call_id: String, args_delta: String },
    /// 工具调用完成
    ToolCallEnd { call_id: String },
    /// 工具执行结果
    ToolResult { call_id: String, result: String },
    /// Token 用量更新
    UsageUpdate { prompt_tokens: u32, completion_tokens: u32 },
    /// 自定义消息
    Custom { key: String, value: serde_json::Value },
}
```

#### `AgentExecutor` — IAgent 到 IExecutor 的桥接

```rust
pub struct AgentExecutor {
    id: String,
    agent: Arc<dyn IAgent>,
    is_output: bool,
}

#[async_trait]
impl IExecutor for AgentExecutor {
    fn id(&self) -> &str { &self.id }

    async fn handle(
        &self,
        message: Box<dyn Any + Send>,
        ctx: &dyn IWorkflowContext,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        // 1. 从 message 提取 ChatMessage（或从 IWorkflowContext 获取历史）
        // 2. 调用 agent.run(messages, session, options)
        // 3. 逐帧消费 stream：
        //    - Text/Reasoning → progress.send(NodeProgress::TextDelta(...))
        //    - ToolCallStart/Args/End → progress.send(NodeProgress::ToolCallXxx(...))
        //    - Usage → progress.send(NodeProgress::UsageUpdate(...))
        // 4. 收集最终 AgentResponse
        // 5. 返回 HandlerResult::Messages(vec![...])
    }
}
```

**流式全链路保证**：Agent 的 `BoxStream<AgentResponseResult>` 在 AgentExecutor 内部被逐帧消费，每一帧通过 `progress` channel 向上透传，引擎统一包装为 `WorkflowEvent::NodeStreaming` 对外广播。**不收集后一次性返回**。

#### `FunctionExecutor<F, I, O>`

```rust
pub struct FunctionExecutor<F, I, O> {
    id: String,
    handler: F,
    _phantom: PhantomData<(I, O)>,
}
```

用于条件分支、路由决策等纯逻辑节点。

#### `TypeTag`

```rust
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeTag {
    pub type_name: String,
    pub type_version: Option<u32>,
}

pub trait ITypeTagged {
    fn type_tag() -> TypeTag;
}
```

### 4.3 builder/ — 构建器层

```rust
pub struct WorkflowBuilder {
    nodes: HashMap<String, Node>,
    edges: Vec<Edge>,
    ports: Vec<RequestPort>,
    output_node_ids: HashSet<String>,
    start_node_id: Option<String>,
}

impl WorkflowBuilder {
    pub fn new() -> Self;

    // 注册节点
    pub fn add_node(mut self, id: impl Into<String>, executor: Arc<dyn IExecutor>) -> Self;
    pub fn add_agent_node(mut self, id: impl Into<String>, agent: Arc<dyn IAgent>) -> Self;

    // 入口 / 输出
    pub fn set_start(mut self, id: impl Into<String>) -> Self;
    pub fn with_output_from(mut self, id: impl Into<String>) -> Self;

    // 边
    pub fn add_edge(mut self, source: impl Into<String>, target: impl Into<String>) -> Self;
    pub fn add_fan_out_edge(mut self, source: impl Into<String>, targets: Vec<String>) -> Self;
    pub fn add_fan_in_edge(mut self, sources: Vec<String>, target: impl Into<String>) -> Self;

    pub fn add_port(mut self, port: RequestPort) -> Self;

    // 构建
    pub fn build(self) -> Result<WorkflowGraph>;
}
```

### 4.4 engine/ — 执行引擎 + 全链路事件系统（核心）

#### `WorkflowEvent` 枚举 — RAF 的可观测性核心

```rust
/// 工作流事件 — 全生命周期 + 节点级粒度
///
/// 前端可逐事件消费，实现：
/// - DAG 图实时高亮
/// - 每个 Agent 的状态徽标
/// - 每个 Agent 的实时打字机输出
/// - 多 Agent 并行进度条
#[derive(Clone, Serialize)]
#[serde(tag = "event_type", content = "data")]
pub enum WorkflowEvent {
    /// 工作流启动
    WorkflowStarted {
        session_id:      String,
        graph_node_ids:  Vec<String>,
        start_node_id:   String,
        timestamp:       DateTime<Utc>,
    },

    // ── SuperStep 生命周期 ──

    SuperStepStarted {
        step_number:     i32,
        active_nodes:    Vec<String>,   // 本轮将被激活的节点
        timestamp:       DateTime<Utc>,
    },
    SuperStepCompleted {
        step_number:     i32,
        outputs_count:   usize,
        timestamp:       DateTime<Utc>,
    },

    // ── 节点生命周期 ──

    /// 节点收到消息，即将开始执行
    NodeInvoking {
        node_id:         String,
        node_name:       String,        // display name
        step_number:     i32,
        timestamp:       DateTime<Utc>,
    },
    /// 节点流式输出增量 — 最核心的前端交互事件
    NodeStreaming {
        node_id:         String,
        chunk:           NodeChunk,     // TextDelta / ToolCall* / ReasoningDelta / Usage
        timestamp:       DateTime<Utc>,
    },
    /// 节点执行成功
    NodeCompleted {
        node_id:         String,
        messages_produced: usize,      // 产出了多少条下游消息
        usage:           Option<UsageInfo>,
        timestamp:       DateTime<Utc>,
    },
    /// 节点执行失败
    NodeFailed {
        node_id:         String,
        error:           String,
        timestamp:       DateTime<Utc>,
    },

    // ── 输出 / 终止 ──

    /// 工作流产出了最终响应
    AgentResponse {
        node_id:         String,
        response:        ChatMessage,
        timestamp:       DateTime<Utc>,
    },
    /// 工作流正常完成
    WorkflowCompleted {
        total_steps:     i32,
        total_nodes:     usize,
        total_usage:     Option<UsageInfo>,
        timestamp:       DateTime<Utc>,
    },
    /// 工作流错误
    WorkflowError {
        error:           String,
        node_id:         Option<String>,  // 发生在哪个节点
        timestamp:       DateTime<Utc>,
    },
}

/// 节点流式块 — 映射 NodeProgress → 可序列化事件
#[derive(Clone, Serialize)]
#[serde(tag = "chunk_type")]
pub enum NodeChunk {
    TextDelta { delta: String },
    ReasoningDelta { delta: String },
    ToolCallStart { call_id: String, name: String },
    ToolCallArgs { call_id: String, args_delta: String },
    ToolCallEnd { call_id: String },
    ToolResult { call_id: String, result: String },
    UsageUpdate { prompt_tokens: u32, completion_tokens: u32 },
    Custom { key: String, value: serde_json::Value },
}

#[derive(Clone, Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
```

#### `WorkflowEngine`

```rust
pub struct WorkflowEngine {
    graph:              Arc<WorkflowGraph>,
    state_store:        Arc<StateStore>,
    checkpoint_manager: Option<Arc<dyn ICheckpointManager>>,
    edge_runners:       HashMap<EdgeId, Box<dyn IEdgeRunner>>,
    event_tx:           tokio::sync::broadcast::Sender<WorkflowEvent>,
}

impl WorkflowEngine {
    pub fn new(graph: WorkflowGraph) -> Self;

    // ═══ 核心 API ═══

    /// 完整运行，返回事件流 + 最终输出流
    ///
    /// 返回 `(BoxStream<WorkflowEvent>, BoxStream<Result<WorkflowOutput>>)`。
    /// - 事件流：包含所有生命周期事件（NodeInvoking/NodeStreaming/NodeCompleted…）
    /// - 输出流：包含工作流产出的最终消息
    /// 前端同时订阅两个流即可实现全链路可观测 + 最终输出消费。
    pub async fn run(
        &self,
        initial_message: Box<dyn Any + Send>,
        session: Option<Arc<dyn ISession>>,
    ) -> Result<(
        BoxStream<'static, WorkflowEvent>,
        BoxStream<'static, Result<WorkflowOutput>>,
    )>;

    /// 单步执行（用于检查点控制）
    pub async fn execute_super_step(
        &self,
        step_ctx: &mut StepContext,
    ) -> Result<SuperStepResult>;

    /// 从检查点恢复，复用同一个事件/输出流模型
    pub async fn resume(
        &self,
        checkpoint: Checkpoint,
    ) -> Result<(
        BoxStream<'static, WorkflowEvent>,
        BoxStream<'static, Result<WorkflowOutput>>,
    )>;

    /// 仅订阅事件流（不关心最终输出）
    pub fn subscribe_events(&self) -> BoxStream<'static, WorkflowEvent>;
}

pub enum SuperStepResult {
    Completed { outputs: Vec<WorkflowOutput> },
    Halted    { pending_requests: Vec<ExternalRequest> },
    InProgress { step_ctx: StepContext },
}

pub struct WorkflowOutput {
    pub node_id: String,
    pub content: Box<dyn Any + Send>,
}
```

#### SuperStep 执行流程 + 事件发射点

```
execute_super_step:
  1. Advance → 创建新的 StepContext
  2. Emit SuperStepStarted { step_number, active_nodes }
  3. Deliver Messages:
     a. 遍历 StepContext.queued_messages
     b. EdgeRunner::chase() → DeliveryMapping
     c. 分配到各节点
  4. Execute (并行):
     对每个有消息的节点：
     a. Emit NodeInvoking { node_id }
     b. 创建 mpsc channel (progress_tx, progress_rx)
     c. spawn: IExecutor::handle(message, ctx, progress_tx)
     d. spawn: 从 progress_rx 读取 NodeProgress，包装为 WorkflowEvent::NodeStreaming 广播
     e. 等待 handle 完成
     f. 成功 → Emit NodeCompleted { node_id, messages_produced, usage }
     g. 失败 → Emit NodeFailed { node_id, error }
  5. Publish State
  6. Checkpoint
  7. Emit SuperStepCompleted { step_number, outputs_count }
```

**关键设计**：每个节点的 `progress_tx → progress_rx → WorkflowEvent::NodeStreaming` 管道在 spawn 的独立 task 中运行，确保引擎在等待 `handle()` 完成的同时，流式进度事件已实时广播给前端。

#### `IEdgeRunner` trait

```rust
#[async_trait]
pub trait IEdgeRunner: Send + Sync {
    async fn chase(
        &self,
        envelope: &MessageEnvelope,
        nodes: &HashMap<String, Arc<dyn IExecutor>>,
    ) -> Result<Vec<MessageDelivery>>;
}

struct DirectEdgeRunner  { edge_data: DirectEdgeData }
struct FanOutEdgeRunner  { edge_data: FanOutEdgeData }
struct FanInEdgeRunner   { edge_data: FanInEdgeData,  state: Mutex<FanInState> }
```

#### `IWorkflowContext` — Executor 的服务接口

```rust
#[async_trait]
pub trait IWorkflowContext: Send + Sync {
    /// 发送消息到下游节点
    async fn send_message(&self, message: Box<dyn Any + Send>, target_id: Option<&str>);

    /// 产出工作流输出（不沿边，直接 yield 给调用者）
    async fn yield_output(&self, output: Box<dyn Any + Send>);

    /// 请求暂停（外部请求等待）
    async fn request_halt(&self);

    /// 读取当前步骤的状态
    async fn read_state<T: DeserializeOwned>(&self, key: &str, scope: Option<&str>) -> Result<Option<T>>;

    /// 写入状态（延迟发布，SuperStep 结束时提交）
    async fn write_state<T: Serialize + Send>(&self, key: &str, value: T, scope: Option<&str>) -> Result<()>;

    /// 清除状态
    async fn clear_state(&self, key: &str, scope: Option<&str>) -> Result<()>;

    /// 当前节点 ID
    fn current_node_id(&self) -> &str;

    /// 当前步骤编号
    fn step_number(&self) -> i32;

    /// 获取外部会话
    fn session(&self) -> Option<&Arc<dyn ISession>>;
}
```

注意：`IWorkflowContext` 不再有 `emit_event()` — 事件由引擎通过 progress channel 统一管理，避免 Executor 直接操作事件广播造成混乱。

#### `MessageEnvelope`

```rust
pub struct MessageEnvelope {
    pub message_id:     String,
    pub source_node_id: String,
    pub target_node_id: Option<String>,
    pub content:        Box<dyn Any + Send>,
    pub type_tag:       TypeTag,
    pub metadata:       HashMap<String, serde_json::Value>,
    pub created_at:     DateTime<Utc>,
}
```

### 4.5 state/ — 状态管理层

```rust
pub struct StateStore {
    scopes:         RwLock<HashMap<ScopeId, HashMap<String, serde_json::Value>>>,
    queued_updates: RwLock<HashMap<UpdateKey, StateUpdate>>,
}

pub enum StateUpdate {
    Set { key: String, value: serde_json::Value },
    Delete { key: String },
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ScopeId {
    pub node_id: String,
    pub scope_name: Option<String>, // None = 私有
}
```

### 4.6 checkpoint/ — 检查点层

```rust
#[derive(Serialize, Deserialize)]
pub struct Checkpoint {
    pub step_number:      i32,
    pub graph_hash:       String,
    pub state_data:       HashMap<ScopeKey, serde_json::Value>,
    pub edge_state_data:  HashMap<String, serde_json::Value>,
    pub step_context:     Option<SerializableStepContext>,
    pub created_at:       DateTime<Utc>,
}

#[async_trait]
pub trait ICheckpointStore: Send + Sync {
    async fn save(&self, session_id: &str, checkpoint: Checkpoint) -> Result<CheckpointInfo>;
    async fn load(&self, session_id: &str, info: &CheckpointInfo) -> Result<Checkpoint>;
    async fn list(&self, session_id: &str) -> Result<Vec<CheckpointInfo>>;
    async fn delete(&self, session_id: &str, info: &CheckpointInfo) -> Result<()>;
}

#[async_trait]
pub trait ICheckpointManager: Send + Sync {
    async fn commit(&self, session_id: &str, checkpoint: Checkpoint) -> Result<CheckpointInfo>;
    async fn lookup(&self, session_id: &str, info: &CheckpointInfo) -> Result<Checkpoint>;
    async fn list(&self, session_id: &str) -> Result<Vec<CheckpointInfo>>;
}
```

内置实现：`InMemoryCheckpointStore`、`JsonFileCheckpointStore`。

### 4.7 patterns/ — 编排模式层

所有模式内部改用 `WorkflowEngine` 驱动，对外 `run()` 返回 `(BoxStream<WorkflowEvent>, BoxStream<Result<WorkflowOutput>>)`：

| 模式 | Engine 内部图结构 |
|------|------------------|
| `SequentialPattern` | Agent₁ → DirectEdge → Agent₂ → DirectEdge → … → Agentₙ |
| `ConcurrentPattern` | source → FanOutEdge → [Agent₁, …, Agentₙ] → FanInEdge → output |
| `HandoffPattern` | HandoffStart → FanOutEdge(conditional) → [Agent₁, …, Agentₙ] → HandoffEnd |
| `GroupChatPattern`（新增） | GroupChatHost → FanOutEdge → [Agent₁, …, Agentₙ]→ FanInEdge → GroupChatHost（循环） |

```rust
pub struct GroupChatPattern {
    agents:      Vec<Arc<dyn IAgent>>,
    selector:    Box<dyn ISpeakerSelector>,
    termination: Box<dyn ITerminationCondition>,
    max_turns:   usize,
}

pub trait ISpeakerSelector: Send + Sync {
    fn select_next(&self, history: &[ChatMessage], agents: &[Arc<dyn IAgent>]) -> Result<usize>;
}

pub trait ITerminationCondition: Send + Sync {
    fn should_terminate(&self, history: &[ChatMessage]) -> bool;
}
```

### 4.8 `WorkflowAgent` — 将 Engine 包装为 IAgent

对外统一接口：任何 workflow 都可以作为 Agent 嵌入更大的编排中：

```rust
pub struct WorkflowAgent {
    engine: Arc<WorkflowEngine>,
    id: AgentId,
    metadata: AgentMetadata,
}

#[async_trait]
impl IAgent for WorkflowAgent {
    // run() 内部调用 engine.run()
    // 收集 WorkflowOutput 流，转换为 AgentResponseResult 流
    // 同时将 WorkflowEvent 流通过 session 的 provider_state 发布（可选）
}
```

### 4.9 流式全链路示意

```
前端订阅:
  ┌─────────────────────────────────────────────────────────┐
  │  event_stream.subscribe()                               │
  │  ┌─────────────────────────────────────────────────────┐│
  │  │ WorkflowStarted                                     ││
  │  │ SuperStepStarted { active_nodes: ["researcher"] }   ││
  │  │ NodeInvoking { node_id: "researcher" }              ││
  │  │ NodeStreaming { chunk: TextDelta("正在搜索...") }    ││  ← 打字机效果
  │  │ NodeStreaming { chunk: ToolCallStart("search") }    ││  ← 工具调用状态
  │  │ NodeStreaming { chunk: ToolResult("找到3条结果") }   ││
  │  │ NodeCompleted { node_id: "researcher" }             ││
  │  │ SuperStepCompleted                                  ││
  │  │ SuperStepStarted { active_nodes: ["writer"] }       ││
  │  │ NodeInvoking { node_id: "writer" }                  ││
  │  │ NodeStreaming { chunk: TextDelta("根据研究...") }    ││
  │  │ NodeCompleted { node_id: "writer" }                 ││
  │  │ AgentResponse { response: ChatMessage(...) }       ││  ← 最终输出
  │  │ WorkflowCompleted                                   ││
  │  └─────────────────────────────────────────────────────┘│
  └─────────────────────────────────────────────────────────┘
```

前端可由此构建：
- DAG 图实时高亮：`NodeInvoking` → 节点变色，`NodeCompleted` → 节点绿色
- Agent 打字机输出：`NodeStreaming(chunk)` → 追加文本
- 工具调用卡片：`NodeStreaming(ToolCallStart)` → 显示工具卡片
- Token 用量仪表：`NodeCompleted(usage)` 累计各节点消耗

---

## 5. 实施阶段

### 阶段 1：基础图结构 + IExecutor 抽象 + 事件类型

**文件清单：**
- `crates/workflow/src/graph/` — 新建：`mod.rs`, `workflow_graph.rs`, `edge.rs`, `edge_data.rs`, `node.rs`
- `crates/workflow/src/executor/` — 新建：`mod.rs`, `base.rs`, `agent_executor.rs`, `function_executor.rs`
- `crates/workflow/src/engine/event.rs` — 新建：`WorkflowEvent` 枚举、`NodeChunk`、`NodeProgress`
- `crates/workflow/src/lib.rs` — 更新导出
- `crates/workflow/Cargo.toml` — 添加 `parking_lot`、`chrono`、`uuid` 依赖

**产出：**
- `WorkflowGraph` + `Edge` 枚举
- `IExecutor` trait（含 `progress` channel 参数）
- `AgentExecutor`（流式桥接 IAgent）
- `FunctionExecutor`（轻量节点）
- `WorkflowEvent` / `NodeChunk` / `NodeProgress` 完整类型

### 阶段 2：Builder

**产出：** `WorkflowBuilder` + `build()` 验证

### 阶段 3：执行引擎（含全链路事件）

**产出：**
- `WorkflowEngine` + SuperStep 循环（含事件发射点）
- `IEdgeRunner` x3 实现
- `StepContext` + `IWorkflowContext`
- `MessageEnvelope` + `MessageRouter`

### 阶段 4：状态管理 + 检查点

**产出：** `StateStore`、`ICheckpointStore`、`ICheckpointManager`、InMemory 实现

### 阶段 5：模式迁移 + 新增

**产出：**
- `SequentialPattern` / `ConcurrentPattern` / `HandoffPattern` 基于 Engine 重构
- `GroupChatPattern` 新增
- `WorkflowAgent`（Engine → IAgent 包装）
- 旧 API 保留向后兼容一个版本

### 阶段 6：宏 + 类型注册（后续按需）

**产出：** `#[derive(ITypeTagged)]` proc macro

---

## 6. 依赖变更

Workspace `Cargo.toml` 新增：
```toml
chrono = { version = "0.4", features = ["serde"] }
parking_lot = "0.12"
uuid = { version = "1", features = ["v4"] }
```

`crates/workflow/Cargo.toml` 新增：
```toml
parking_lot = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
```

---

## 7. 验证方案

### 7.1 编译验证
```bash
cargo check -p rust-agent-workflow
cargo clippy -p rust-agent-workflow -- -D warnings
```

### 7.2 单元测试

| 测试 | 验证点 |
|------|--------|
| `test_build_simple_linear_graph` | 两个节点一条边，build 成功 |
| `test_build_missing_node_fails` | 边引用了不存在的节点，build 返回错误 |
| `test_super_step_single_delivery` | 一条消息 → 一个节点 → 事件流包含 NodeInvoking/Streaming/Completed |
| `test_super_step_fan_out` | FanOut → 并行三个节点 → FanIn 合并 |
| `test_super_step_fan_in_barrier` | FanIn 等待全部源到达后才释放 |
| `test_event_stream_contains_lifecycle` | `run()` 返回的事件流包含完整的 WorkflowStarted → … → WorkflowCompleted |
| `test_node_streaming_during_execution` | NodeStreaming 事件在 handle() 执行期间实时到达，而非完成后批量 |
| `test_state_write_and_read` | 写入→发布→读取一致 |
| `test_checkpoint_save_and_resume` | 暂停→保存→恢复→继续→结果一致 |
| `test_sequential_pattern_via_engine` | SequentialPattern 内部使用 Engine，事件流顺序正确 |
| `test_group_chat_roundrobin` | 3 agent 轮流发言→终止→事件流包含每个 agent 的 NodeInvoking/Completed |

### 7.3 集成验证
- 编写示例：使用 `AgentBuilder` 创建 3 个 Agent → `WorkflowBuilder` 构建 FanOut 图 → 订阅事件流打印节点状态 → 验证输出
- 与 `crates/cli` 集成：终端中显示当前活跃节点和实时文本输出

---

## 8. 关键设计决策

1. **`IExecutor::handle()` 通过 `progress` channel 输出增量进度**：而不是让 Executor 直接知道 `WorkflowEvent`。引擎统一控制事件包装和广播，保持 Executor 关注点单一。

2. **`WorkflowEngine::run()` 返回双流 `(EventStream, OutputStream)`**：事件流用于前端交互，输出流用于最终结果消费。两个流独立订阅，互不阻塞。

3. **`WorkflowEvent::NodeStreaming` 携带 `node_id`**：前端可以按节点分组展示流式输出，实现多 Agent 并行时各自独立的打字机效果。

4. **所有 trait 统一 `I` 前缀**：与现有代码库的 `IAgent`、`ISession`、`IChatClient`、`IContextProvider`、`ITool` 保持一致。

5. **MAF 的 `ExecutorBinding` 隐式转换→Rust 枚举显式表达**：Rust 无隐式转换，用枚举区分 Instance/Factory/Placeholder。

6. **MAF 的 CAS 所有权→`Arc<dyn IExecutor>`**：Rust 的 `Arc` 天然提供共享访问。

7. **MAF 的 `Type` 反射→`TypeTag` + proc macro**：Rust 无运行时反射，用字符串标识符 + derive 宏替代。

8. **进程内执行优先，分布式后置**：阶段 1-6 均为进程内。

---

## 9. 风险与注意事项

- **双流返回的 API 复杂度**：`run()` 返回 `(event_stream, output_stream)`，调用者需了解两个流的消费模式。提供 `subscribe_events()` 和 `WorkflowAgent`（包装为 IAgent）作为简化入口。
- **`NodeStreaming` 事件频率**：LLM 流式输出可能高频产生事件，需确保 `broadcast::channel` 容量充足（建议 1024），或提供可选的聚合模式（如 50ms 窗口内合并 TextDelta）。
- **AgentExecutor 的流式桥接**：IAgent 的 `BoxStream<AgentResponseResult>` 逐帧转为 `NodeProgress`，需正确映射 Content 枚举的 12 种变体到 NodeChunk。
- **FanIn 栅栏状态的检查点序列化**：需确保 `FanInState` 可序列化。
- **API 向后兼容**：旧 `SequentialPattern::new().run()` 签名变更（返回 `BoxStream<Result<AgentResponseResult>>` → 返回双流）。提供 `SequentialPattern::run_simple()` 收集输出并丢弃事件的兼容方法。
- **proc macro 编译时间**：`ITypeTagged` derive 宏应轻量。

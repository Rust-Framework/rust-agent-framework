# Rust Agent Framework -- 完整 Agent 请求/响应管道构建计划

> 参考：Microsoft MAF (.NET) `DelegatingAIAgent` 装饰器管道 + Workflow SuperStep 引擎架构
>
> **重要前置原则**：MAF 模式需要 Rust 惯用适配，不是 1:1 移植。详见「零、MAF 模式 Rust 适配分析」。

---

## 零、MAF 模式 Rust 适配分析（关键前置问题）

### 0.1 核心结论：选择性适配，分层处理

MAF 是 C# 17 框架，大量使用接口继承、`AsyncLocal<T>`、DI 容器、运行时类型擦除等 .NET 原生特性。这些模式在 Rust 中需要重新审视 -- **某些模式直接适配是合理的，某些需要替换为 Rust 惯用等价物，某些则根本不应引入。**

### 0.2 逐模式对照

| MAF .NET 模式 | 是否违背 Rust 最佳实践 | 适配方案 | 理由 |
|---|---|---|---|
| `DelegatingAIAgent` 装饰器链 | **否。Trait + Wrapper Struct 是 Rust 惯用法** | `Arc<dyn Agent>` 嵌套，Newtype 模式 | Tower Service/Layer、Axum middleware 均用此模式；动态分发在管道外层是必要的 |
| `IChatClient` 内部装饰器链 | **部分。建议用 Tower** | ChatClient 层引入 `tower::Service`，免费获得 `TimeoutLayer`、`RetryLayer`、`RateLimitLayer` | Tower 是 Rust 异步中间件的工业标准 |
| `AsyncLocal<AgentRunContext>` | **是。Rust 无等价物，不应模仿** | 显式 `AgentContext` 结构体作为参数传入；`tracing::Span` 仅用于观测传播 | 隐式全局状态违反 Rust 编译期安全哲学，且测试隔离困难 |
| DI 容器 / `IServiceProvider` | **是。不应引入** | 编译期泛型注入 + Builder 模式 + Workspace 工厂函数 | Rust 编译期单态化天然提供 DI；运行时 `HashMap<TypeId, Box<dyn Any>>` 引入 panic 风险 |
| `IChatClient` + `IStreamingChatClient` 多 trait | **部分。应合并** | 单一 `CompletionProvider` trait + 泛型方法 + 默认实现 | 一个领域概念 = 一个 trait；不同行为通过方法参数/泛型表达 |
| `AgentSessionStore` 体系 | **否。适配良好** | `ISession` trait + `AgentSession`(内存) + 可扩展为 Redis/DB impl | Rust trait object 天然适合此场景 |
| `ChatHistoryProvider` + `AIContextProvider` | **否。适配良好** | 合并到 `HistoryAgent` 装饰器 | 前置/后置拦截语义清晰，装饰器模式直接映射 |
| Workflow `Executor` + `MessageRouter` | **否。适配良好** | SuperStep + Edge 路由 + `WorkflowBuilder` | 图执行是数据驱动，用 `enum Edge` + 消息队列是 Rust 惯用法 |
| `WorkflowHostAgent` (桥接两层管道) | **否。适配良好** | 实现 `Agent` trait 的 Workflow 包装器 | 桥接层价值明确：Workflow 对外表现为普通 Agent |

### 0.3 必须避免的 Rust 反模式

| 反模式 | 说明 | 典型案例 |
|---|---|---|
| DI 容器 / Service Locator | 运行时 `resolve::<dyn Trait>()` -- panic 风险 + 无编译期检查 | `HashMap<TypeId, Box<dyn Any>>` |
| 隐式全局可变状态 | 测试隔离困难、并发语义不透明、panic 后不确定 | `lazy_static!` + `Mutex` 存储业务状态 |
| `async_trait` 过度使用 | 每个方法调用一次堆分配 (`Box<dyn Future>`) | 简单场景（< 5 个 impl）考虑 `enum` + `match` |
| `Arc<dyn Trait>` 深层嵌套 | 每层 Arc 原子计数 + vtable 间接跳转 | ChatClient 层优先泛型，仅管道外层用 Arc |
| 1:1 移植 OOP 继承层级 | Rust trait 不支持继承链 | 用组合 + trait 默认实现替代多层继承 |
| `tokio::task_local!` 隐藏业务上下文 | 编译期不可见，运行时可能 None panic | 仅用于纯内部实现细节，不暴露为公共 API |

### 0.4 分层采用策略

```
                    ┌──────────────────────────────┐
                    │     Agent 管道外层            │
                    │   Arc<dyn Agent> 动态分发     │  ← 运行时组合，动态分发必要
                    │   (DelegatingAgent 装饰器链)   │
                    ├──────────────────────────────┤
                    │     ChatClient 层             │
                    │   tower::Service 泛型静态分发  │  ← 编译时确定 provider 类型
                    │   (Timeout/Retry/RateLimit)   │  ← 复用 Tower 生态免费中间件
                    ├──────────────────────────────┤
                    │     Workflow 层               │
                    │   enum Edge + 消息队列        │  ← 数据驱动，零动态分发
                    │   (SuperStep 屏障模型)         │
                    └──────────────────────────────┘
```

---

## 一、当前状态总结

### 1.1 已具备的基础设施

| 模块 | 状态 | 说明 |
|------|------|------|
| `IAgent` trait | 完整 | `run()` 返回 `BoxStream<AgentStreamChunk>` |
| `IChatClient` trait | 完整 | SSE 流式传输，OpenAI/DeepSeek 已实现 |
| `IMiddleware` trait | 部分 | `on_request` / `on_response` 均已定义，但 `on_response` 从未被调用 |
| `ChatClientAgent` | 完整 | 单一具体 Agent，组装 middleware + chat_client + tools + history |
| Tool 系统 | 完整 | `#[tool]` 过程宏 + `ToolRegistry` + JSON Schema 生成 |
| 消息类型体系 | 完整 | `ChatMessage` / `ChatStreamChunk` / `AgentStreamChunk` / `AgentResponse` |
| Agent 运行时 | 基础 | `AgentRuntime` 仅做注册/路由，无生命周期管理 |
| Workflow | 骨架 | `GraphFlow` 无 edge/condition；`HandoffPattern` 占位 |

### 1.2 与 MAF .NET 的核心差距

| MAF .NET 能力 | Rust 框架当前状态 | 差距等级 |
|---|---|---|
| `DelegatingAIAgent` 装饰器管道 | 无。middleware 仅做前置变换 | **高** |
| Agent Loop（tool calling 自动循环） | 无。框架无法处理 tool_calls 并回传结果 | **高** |
| `ChatHistoryProvider` / `AIContextProvider` | 无。两套 session 机制并存且不关联 | **高** |
| `AgentRunContext` (AsyncLocal 传递) | 无。无运行上下文传播机制 | **中** |
| IChatClient 内部装饰器链 | 无。ChatClient 是扁平实现 | **中** |
| Workflow SuperStep + Edge 路由 | `GraphFlow` 不支持 edges/conditions | **高** |
| Checkpoint / Resume | 无 | **中** |
| 非流式调用 | 不支持 | **低** |
| 可观测性 (tracing/metrics) | 仅有 1 处 `tracing::debug!` | **中** |
| DI / Builder 模式 | 无 | **中** |

---

## 二、目标架构

分 **三个阶段** 构建，优先级从高到低：

### 阶段 1：核心 Agent 管道（对应 MAF DelegatingAIAgent + Agent Loop）

```
调用方
  │
  ▼
┌────────────────────────────────────────────────────────────────────┐
│  Agent::run(ctx: &AgentContext, messages)  -- 统一入口             │
│                                                                    │
│  1. AgentContext 显式传入（编译期保证非空，替代 AsyncLocal）        │
│                                                                    │
│  2. DelegatingAgent 装饰器链 (责任链模式)                          │
│     ┌──────────────────┐                                          │
│     │  TracingAgent    │  ← 可观测性：tracing::Span + metrics      │
│     │  inner ↓         │                                          │
│     │  ToolLoopAgent   │  ← Agent Loop：自动 tool calling 循环     │
│     │  inner ↓         │                                          │
│     │  HistoryAgent    │  ← 历史管理：从 ISession 加载/持久化      │
│     │  inner ↓         │                                          │
│     │  ChatClientAgent │  ← 终端节点：调用 CompletionProvider      │
│     │    └─ Tower      │     （内部使用 tower::Service 获得重试等） │
│     └──────────────────┘                                          │
│                                                                    │
│  3. 返回 BoxStream<AgentChunk>                                    │
└────────────────────────────────────────────────────────────────────┘
```

#### 关键新增/修改

##### A. `DelegatingAgent` -- 装饰器基类（新增）

**文件**：`crates/core/src/delegating_agent.rs`（新建）

```rust
/// 装饰器模式基类 -- 对应 MAF 的 DelegatingAIAgent
/// 所有方法默认透传给 inner_agent，子类覆写以注入行为
pub struct DelegatingAgent {
    inner_agent: Arc<dyn IAgent>,
}

impl DelegatingAgent {
    pub fn new(inner: Arc<dyn IAgent>) -> Self;
    pub fn inner(&self) -> &Arc<dyn IAgent>;
}

#[async_trait]
impl IAgent for DelegatingAgent {
    async fn run(...) -> BoxStream<AgentStreamChunk> {
        // 默认透传，子类可在覆写时插入前置/后置逻辑
        self.inner_agent.run(messages, session, options).await
    }
    fn id(&self) -> &AgentId;
    fn metadata(&self) -> &AgentMetadata;
    async fn reset(&self);
}
```

##### B. `AgentContext` -- 显式运行上下文（新增）

**文件**：`crates/core/src/context.rs`（新建）

```rust
/// 显式传递的运行上下文 — Rust 惯用替代 AsyncLocal
/// 编译期保证：函数签名中缺少 ctx 参数 = 编译错误（而非运行时 None panic）
pub struct AgentContext {
    pub run_id: Uuid,
    pub agent_id: AgentId,
    pub session: Option<Arc<dyn ISession>>,
    pub options: ChatAgentRunOptions,
}

impl AgentContext {
    pub fn new(
        agent_id: AgentId,
        session: Option<Arc<dyn ISession>>,
        options: ChatAgentRunOptions,
    ) -> Self { ... }
}
```

**`Agent` trait 签名**：
```rust
#[async_trait]
pub trait Agent: Send + Sync {
    /// ctx 必须显式传入 — 编译期强制非空
    async fn run(
        &self,
        ctx: &AgentContext,
        messages: Vec<ChatMessage>,
    ) -> BoxStream<'static, Result<AgentChunk, AgentError>>;
}
```

> **设计原则**：
> - 业务状态（session、options、agent_id）通过 `ctx` 参数显式传入 —— 编译期安全检查
> - 观测数据（span、trace_id）通过 `tracing::Span` 自动传播 —— 运行时自动关联
> - 两者不混淆：业务状态绝不塞入 Span Extensions
> - 这与 MAF 的 `AsyncLocal<AgentRunContext>` 不同：MAF 隐式传播，Rust 显式传递。但语义等价——链中任何组件都能访问当前运行的完整上下文。

##### C. `ToolLoopAgent` -- Agent Loop 中间件（新增）

**文件**：`crates/framework/src/agents/tool_loop_agent.rs`（新建）

这是最关键的缺失能力 -- 自动 tool calling 循环：

```
loop {
    chat_client.run(messages) -> stream
    collect chunks -> AgentResponse
    if response.tool_calls.is_empty() { break }
    for tool_call in response.tool_calls {
        result = tool_registry.execute(tool_call)
        messages.push(ToolResult { tool_call_id, content: result })
    }
    messages.push(Assistant { tool_calls })
}
return final response
```

**设计要点**：
- 最大循环次数限制（默认 10 次，可配置）
- 支持并行工具调用（`tokio::join_all`）
- 累积所有轮次的 tool_calls 到最终响应
- 支持 early break 条件（如 model 返回 stop_reason）

##### D. `HistoryAgent` -- 历史管理中间件（新增）

**文件**：`crates/framework/src/agents/history_agent.rs`（新建）

对应 MAF 的 `ChatHistoryProvider`：
- `run()` 前置：从 `ISession` 加载历史消息，合并到当前 messages
- `run()` 后置：将新的 User/Assistant/Tool 消息写入 `ISession`
- 统一现有的两套 session（`ChatClientAgent.history` + `AgentSession`）为单一 `ISession`

##### E. `TracingAgent` -- 可观测性中间件（新增）

**文件**：`crates/framework/src/agents/tracing_agent.rs`（新建）

- 使用 `tracing` crate 记录：agent_id、耗时、token 消耗、错误
- 使用 `metrics` crate（可选）暴露 prometheus 指标
- span 传播：`info_span!("agent_run", agent_id = %id)`

##### F. `IMiddleware` 重构

**文件**：`crates/core/src/middleware.rs`（修改）

当前 `IMiddleware` 职责与 `DelegatingAgent` 重叠。重构方案：
- 保留 `IMiddleware` 作为轻量级拦截器（仅对 messages 做变换）
- `DelegatingAgent` 作为重量级装饰器（可控制是否调用 inner）
- `IMiddleware` 可以被 `ChatClientAgent` 直接使用（兼容现有 CLI 代码）

##### G. ChatClientAgent 重构

**文件**：`crates/framework/src/chat_client_agent.rs`（修改）

- 移除内置 `history` 字段（由 `HistoryAgent` 管理）
- 移除内置 `middleware` 链（由 `DelegatingAgent` 装饰器链替代）
- 聚焦为纯 "终端 Agent"：接收 messages + options → 调用 `IChatClient` → 返回 stream
- 保留 `tools` 和 `instructions` 字段

##### H. `Agent` trait 扩展（原 `IAgent`）

**文件**：`crates/core/src/agent.rs`（修改）

```rust
/// 原有 IAgent 重命名为 Agent（Rust 惯例：不加 I 前缀）
/// trait 方法签名调整：session 从独立参数移入 AgentContext
#[async_trait]
pub trait Agent: Send + Sync {
    fn id(&self) -> &AgentId;
    fn metadata(&self) -> &AgentMetadata;

    /// 核心方法：ctx 显式传入，编译期保证非空
    async fn run(
        &self,
        ctx: &AgentContext,
        messages: Vec<ChatMessage>,
    ) -> BoxStream<'static, Result<AgentChunk, AgentError>>;

    /// 便捷方法：收集流为非流式响应（对应 MAF 的 RunAsync vs RunStreamingAsync）
    async fn run_sync(
        &self,
        ctx: &AgentContext,
        messages: Vec<ChatMessage>,
    ) -> Result<AgentResponse, AgentError> {
        let stream = self.run(ctx, messages).await;
        collect_agent_response(stream).await
    }

    /// 重置内部状态
    async fn reset(&self);
}
```

---

### 阶段 2：Workflow 图执行引擎（对应 MAF SuperStep + Edge 路由）

```
Workflow
  ├── ExecutorBindings[]    ← 节点（每个绑定一个 Agent）
  ├── Edges[]               ← 有向边（3 种类型）
  │     ├── DirectEdge      ← 1:1 路由
  │     ├── FanOutEdge      ← 1:N 广播
  │     └── FanInEdge       ← N:1 聚合（屏障）
  └── OutputExecutors[]     ← 产出终端

执行模型：SuperStep 屏障同步
  SuperStep N:
    [Executor A] ──msg──▶ [Executor B] ──msg──▶ [Executor C]
    Task::join_all([A, B, C]) → 屏障 → SuperStep N+1
```

#### 新增/修改

##### A. `WorkflowExecutor` -- 工作流执行器（重写 `GraphFlow`）

**文件**：`crates/workflow/src/executor.rs`（重写）

- 替换当前占位的 `GraphFlow`
- 实现 SuperStep 执行模型
- 支持消息队列（每个 Executor 维护 `VecDeque<Message>`）

##### B. `Edge` 类型体系（新增）

**文件**：`crates/workflow/src/edges.rs`（新建）

```rust
pub enum Edge {
    Direct(DirectEdge),
    FanOut(FanOutEdge),
    FanIn(FanInEdge),
}

pub struct DirectEdge {
    pub source_id: ExecutorId,
    pub target_id: ExecutorId,
    pub condition: Option<Box<dyn Fn(&AgentResponse) -> bool + Send + Sync>>,
}
```

##### C. `WorkflowBuilder` -- DSL 构建器（新增）

**文件**：`crates/workflow/src/builder.rs`（新建）

```rust
let workflow = WorkflowBuilder::new(start_agent)
    .add_executor("analyzer", analyzer_agent)
    .add_executor("writer", writer_agent)
    .add_direct_edge("start", "analyzer")
    .add_direct_edge("analyzer", "writer")
    .with_output_from("writer")
    .build()?;
```

##### D. `HandoffPattern` 完善

**文件**：`crates/workflow/src/patterns/handoff.rs`（修改）

- 解析 triage agent 响应中的 target_agent 标识
- 自动路由到目标 agent
- 支持 handoff 链（agent A → agent B → agent C）

##### E. Workflow Checkpoint（可选，低优先级）

- 序列化 Workflow 状态（`ExecutorState` + `MessageQueue`）
- 从 checkpoint 恢复执行
- 使用 `serde` 序列化

---

### 阶段 3：周边基础设施

##### A. Builder 模式 + 配置收敛（新增）

**文件**：`crates/framework/src/builder.rs`（新建）

```rust
// 统一 Agent 构建入口，类似 MAF 的 AddAIAgent() + HostedAgentBuilder
let agent = AgentBuilder::new("my-agent")
    .instructions("You are a helpful assistant.")
    .chat_client(deepseek_client)
    .with_tool(my_tool)
    .with_middleware(tracing_middleware)
    .wrap_with(ToolLoopAgent::new)
    .wrap_with(HistoryAgent::new)
    .build()?;
```

##### B. 可观测性增强

- `tracing` span 覆盖完整管道路径
- OpenTelemetry 导出支持
- 请求级别 metrics（token 消耗、延迟、错误率）

##### C. 非流式调用支持

- `IAgent::run_sync()` 便捷方法（已包含在 trait 扩展中）
- ChatClient 支持 `stream: false` 模式

---

## 三、实施路线图

### 迭代 1：核心管道（优先级最高）

| 步骤 | 文件 | 操作 | 说明 |
|------|------|------|------|
| 1.1 | `crates/core/src/delegating_agent.rs` | **新建** | `DelegatingAgent` 装饰器基类（Newtype 模式：`Arc<dyn Agent>` 嵌套） |
| 1.2 | `crates/core/src/context.rs` | **新建** | `AgentContext` 显式上下文（不引入 task_local，编译期安全） |
| 1.3 | `crates/core/src/agent.rs` | **修改** | `IAgent` → `Agent` 重命名；`run()` 签名改为 `(&self, ctx: &AgentContext, messages)`；新增 `run_sync()` |
| 1.4 | `crates/core/src/lib.rs` | **修改** | 导出新类型；`IAgent` 重导出为 `Agent`（保留 `IAgent` 别名兼容过渡期） |
| 1.5 | `crates/framework/src/agents/` | **新建目录** | 将 agent 相关代码移入子目录 |
| 1.6 | `crates/framework/src/agents/mod.rs` | **新建** | 模块入口 |
| 1.7 | `crates/framework/src/agents/tool_loop_agent.rs` | **新建** | Agent Loop：自动 tool calling 循环（max_rounds 硬上限） |
| 1.8 | `crates/framework/src/agents/history_agent.rs` | **新建** | 历史管理：从 `ISession` 加载/持久化，统一两套 session |
| 1.9 | `crates/framework/src/agents/tracing_agent.rs` | **新建** | 可观测性：`tracing::info_span!` + 耗时记录 |
| 1.10 | `crates/framework/src/chat_client_agent.rs` | **修改** | 重构为终端 Agent：移除 `history`/`middleware`，成为纯 LLM 调用层 |
| 1.11 | `crates/client/` | **修改** | ChatClient 层引入 `tower::Service`，获得 `TimeoutLayer`/`RetryLayer` 免费实现 |
| 1.12 | `crates/framework/src/lib.rs` | **修改** | 更新导出 |
| 1.13 | `crates/cli/src/main.rs` | **修改** | 使用新管道组装 agent（演示 Decorator Chain 组装） |

### 迭代 2：Workflow 引擎

| 步骤 | 文件 | 操作 | 说明 |
|------|------|------|------|
| 2.1 | `crates/workflow/src/executor.rs` | **重写** | SuperStep 执行引擎 |
| 2.2 | `crates/workflow/src/edges.rs` | **新建** | Edge 类型体系 |
| 2.3 | `crates/workflow/src/builder.rs` | **新建** | WorkflowBuilder DSL |
| 2.4 | `crates/workflow/src/graph_flow.rs` | **修改** | 适配新 Executor |
| 2.5 | `crates/workflow/src/patterns/handoff.rs` | **修改** | 完善 handoff 路由 |

### 迭代 3：基础设施

| 步骤 | 文件 | 操作 | 说明 |
|------|------|------|------|
| 3.1 | `crates/framework/src/builder.rs` | **新建** | AgentBuilder 统一入口 |
| 3.2 | `crates/core/src/middleware.rs` | **修改** | 明确与 DelegatingAgent 的职责边界 |

---

## 四、关键设计决策

### 4.1 AsyncLocal 替代方案：显式 AgentContext（非 task_local!）

MAF 使用 `AsyncLocal<AgentRunContext>` 在整个异步调用链中隐式传递上下文。Rust 无直接等价物。

**决策**：显式 `AgentContext` 结构体作为参数传入，**不使用 `tokio::task_local!`**。

| 方案 | 编译期安全 | 性能 | 测试隔离 | 代码可读性 |
|------|-----------|------|---------|-----------|
| 显式 `&AgentContext` 参数 | 缺少参数=编译错误 | 零开销（引用传递） | 天然支持 mock | 签名即文档 |
| `tokio::task_local!` | 运行时 None panic | Future 边界检查开销 | 需手动管理 scope | 隐式依赖，不可见 |
| `tracing::Span` Extensions | 运行时 None，类型需 `Any::downcast_ref` | Span 查找开销 | 需创建 Span 上下文 | 混淆业务与观测 |

**结论**：显式传参是唯一同时满足编译期安全 + 零开销 + 测试友好的方案。`tracing::Span` 仅用于观测数据（`trace_id`、`span_id`）的自动传播，不承载业务状态。

### 4.2 Tower 集成边界

**决策**：ChatClient 层使用 `tower::Service`，Agent 管道层使用自定义 `DelegatingAgent`。

理由：
- ChatClient 是典型的 `Request → Response` 模式，与 Tower 的 `Service<Request>` 语义完全匹配
- 直接获得 `TimeoutLayer`、`RetryLayer`、`RateLimitLayer`、`ConcurrencyLimitLayer` 的免费实现
- Agent 管道层需要双向拦截（`on_request` + `on_response`）+ 条件递归（tool loop），Tower 的 `Layer` 单方向包裹无法表达
- Agent 管道层的 ToolLoopAgent 的递归语义（"条件性地多次调用 inner"）超出了 Tower 的 `poll_ready` + `call` 模型

### 4.3 DelegatingAgent vs IMiddleware 职责划分

| | DelegatingAgent | IMiddleware |
|---|---|---|
| 定位 | 重量级装饰器，可控制是否调用 inner | 轻量级拦截器，仅对数据做变换 |
| 使用场景 | ToolLoopAgent, HistoryAgent, TracingAgent | 消息过滤、敏感词替换、格式转换 |
| 组合方式 | 嵌套 `Arc<dyn IAgent>` | `Vec<Arc<dyn IMiddleware>>` |
| 执行顺序 | 外层 → 内层（责任链） | 调用链中按注册顺序 |

### 4.3 Session 统一方案

当前问题：`ChatClientAgent.history` 和 `AgentSession` 两套机制并存。

**决策**：统一到 `ISession` trait：
- `ChatClientAgent` 不再持有 `history` 字段
- `HistoryAgent` 负责从 `ISession` 加载/持久化
- `AgentSession` 作为默认的 `ISession` 实现（`RwLock<Vec<ChatMessage>>`）
- 用户可实现 `ISession` 对接外部存储（Redis、DB 等）

### 4.4 流式响应中的 on_response 处理

MAF 通过 `ChatHistoryProvider.InvokedAsync()` 在流结束后做后处理。

**决策**：`DelegatingAgent` 在 `run()` 中对 stream 做后处理（如 `HistoryAgent` 将流结束后的最终消息写入 session）：
```rust
async fn run(...) -> BoxStream<AgentStreamChunk> {
    let stream = self.inner.run(...).await;
    // 后处理通过 stream 扩展实现
    stream.chain(once(async { /* post-process */ })).boxed()
}
```

---

## 五、风险评估与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| ToolLoop 无限循环 | 中 | 高 | 硬上限 + 可配置 `max_rounds`（默认 10） |
| 重构破坏现有 CLI | 高 | 中 | `IAgent` → `Agent` 重命名保留 `pub type IAgent<T> = Agent` 别名过渡 |
| SSE streaming 中 tool_call 解析不完整 | 中 | 中 | 参考 OpenAI Python SDK 的 accumulator 模式，ToolLoopAgent 内部做 fragment 聚合 |
| `Arc<dyn Agent>` 装饰器链性能 | 低 | 低 | 管道外层通常仅 3-4 层装饰器，vtable 开销可忽略；ChatClient 内层用泛型/Tower 静态派发 |
| Tower 依赖引入增加编译时间 | 低 | 低 | 仅 `crates/client` 依赖 Tower，不影响 core 抽象层 |
| `async_trait` + `dyn Agent` 的 `Box<dyn Future>` 开销 | 低 | 低 | 当前代码库已使用 `async_trait`；每个 Agent::run 产生一次堆分配，在管道外层（少量装饰器）可接受 |

---

## 六、验证方案

每个迭代完成后通过以下方式验证：

1. **单元测试**：每个新增组件独立测试（`#[cfg(test)]`）
2. **集成测试**：CLI 端到端测试：
   - 单轮对话
   - 多轮对话（history 持久化）
   - tool calling 触发
   - 错误恢复
3. **Workflow 测试**：
   - 顺序编排（A → B → C）
   - 并发编排（fan-out → fan-in）
   - Handoff 路由
4. **可观测性验证**：`RUST_LOG=debug` 确认 trace 输出
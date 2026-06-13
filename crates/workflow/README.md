# rust-agent-workflow

工作流编排层，遵循 MAF 的 graph-based orchestration 设计。

## 功能定位

实现 `IWorkflow` trait，提供多 agent 编排模式，将多个 agent 连接为有向数据流图。

- **GraphFlow**: `IWorkflow` 的图引擎实现
  - agent 节点注册与入口配置
  - 从指定 agent 开始流式执行
- **编排模式**（`patterns/`）:
  - **SequentialPattern**: 顺序执行，前一个 agent 的输出作为下一个的输入
  - **ConcurrentPattern**: 并发执行，多 agent 并行流式输出合并
  - **HandoffPattern**: 交接模式，triage agent 决定路由到哪个目标 agent

## 专属职责

- 实现多 agent 之间的数据流编排
- 提供可复用的编排模式（sequential、concurrent、handoff）
- 管理工作流图的结构（节点、入口点）

## 不做什么

- 不实现 `IAgent`（由 `rust-agent-framework` 负责）
- 不实现 `IChatClient`（由 `rust-agent-client` 负责）
- 不做 checkpointing 或状态持久化
- 不做条件分支或循环控制流（可由外部扩展 `IWorkflow` 实现）
- 不提供 UI 或可视化

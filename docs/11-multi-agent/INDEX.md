# 第 11 章：多智能体编排与业务流程引擎

本章全面介绍 RAF 的多智能体编排系统和业务流程引擎——基于图驱动的工作流引擎，通过六种 Builder 构建专业化编排模式，统一收敛为 IAgent 门面，并在此基础上扩展声明式流程定义（ProcessDefinition）、标准活动节点、SAGA 事务补偿、Agent 团队管理、增强网关、定时调度、消息关联和 SLA 监控等完整的业务基础设施。

本章适合需要构建复杂多 Agent 协作系统和业务流程自动化的架构师和高级开发者。

## 章节目录

| 小节 | 标题 | 内容概要 |
|------|------|---------|
| [11.1](overview.md) | 编排引擎概述 | 图驱动引擎、SuperStep 模型、事件系统、六种编排模式总览 |
| [11.2](sequential-workflow.md) | SequentialWorkflow 顺序编排 | 流水线模式、SequentialWorkflowBuilder、引擎化执行 |
| [11.3](concurrent-workflow.md) | ConcurrentWorkflow 并发编排 | FanOut/FanIn 模式、ConcurrentWorkflowBuilder、并行控制 |
| [11.4](handoff-workflow.md) | HandoffWorkflow 交接编排 | Triage 路由、HandoffWorkflowBuilder、条件边 |
| [11.5](custom-orchestrations.md) | 自定义编排（WorkflowBuilder） | 节点/边/条件、网关 DSL、循环支持、六种执行器 |
| [11.6](checkpoints.md) | 检查点与断点续传 | 增量/全量快照、多存储后端、恢复机制 |
| [11.7](workflow-as-agent.md) | IAgent 统一门面 | Builder→Workflow→as_agent→IAgent 链路、子 Agent 发现、嵌套编排 |
| [11.8](group-chat-workflow.md) | GroupChatWorkflow 群聊编排 | 多轮讨论、ISpeakerSelector、ITerminationCondition |
| [11.9](magentic-workflow.md) | MagenticWorkflow 自主编排 | ReAct 推理循环、Orchestrator 调度、动态任务分解 |
| [11.10](vote-workflow.md) | VoteWorkflow 投票聚合 | 多专家投票、多数决/加权聚合、IVoteAggregator |
| [11.11](process-definition.md) | 流程定义与编译 | 声明式 YAML DSL、16 种 NodeKind、边条件路由、compile() 编译 |
| [11.12](process-instance.md) | 流程实例与状态管理 | 6 状态生命周期、ProcessSnapshot、变量管理、引擎集成 |
| [11.13](standard-activities.md) | 标准活动节点 | 8 种 BPMN 风格 IExecutor：ServiceTask 到 NoneTask |
| [11.14](saga-compensation.md) | SAGA 事务与补偿链 | SagaOrchestrator、BackwardRecovery、ForwardRecovery |
| [11.15](agent-team-pool.md) | Agent 团队与池化管理 | AgentTeam 能力注册、AgentPool 心跳/健康、DynamicRouter 路由 |
| [11.16](advanced-gateways-events.md) | 增强网关、事件与定时调度 | EventBasedGateway、ComplexGateway、BoundaryEvent、TimerTrigger、CronTrigger |
| [11.17](observability.md) | 消息关联、审计与 SLA | MessageCorrelation、IMessageBroker、AuditTrail、SlaTracker |

## 快速导航

- **想了解整体架构？** → [11.1 编排引擎概述](overview.md)
- **想实现简单的 Agent 链？** → [11.2 顺序编排](sequential-workflow.md)
- **想让多个 Agent 并行工作？** → [11.3 并发编排](concurrent-workflow.md)
- **想实现智能路由？** → [11.4 交接编排](handoff-workflow.md)
- **想构建自定义工作流图？** → [11.5 自定义编排](custom-orchestrations.md)
- **想实现断点续传？** → [11.6 检查点系统](checkpoints.md)
- **想统一编排接口？** → [11.7 IAgent 门面](workflow-as-agent.md)
- **想让多个 Agent 讨论？** → [11.8 群聊编排](group-chat-workflow.md)
- **想让 Agent 自主完成任务？** → [11.9 自主编排](magentic-workflow.md)
- **想实现投票决策？** → [11.10 投票聚合](vote-workflow.md)
- **想用 YAML 定义业务流程？** → [11.11 流程定义 DSL](process-definition.md)
- **想管理流程生命周期？** → [11.12 流程实例](process-instance.md)
- **想使用标准活动节点？** → [11.13 标准活动节点](standard-activities.md)
- **想实现分布式事务？** → [11.14 SAGA 补偿](saga-compensation.md)
- **想管理 Agent 团队？** → [11.15 Agent 团队与池化](agent-team-pool.md)
- **想使用增强网关和定时调度？** → [11.16 增强网关与事件](advanced-gateways-events.md)
- **想集成消息中间件和审计？** → [11.17 消息关联与可观测性](observability.md)

## 设计理念

1. **MAF 对齐**：六种 Builder（Sequential / Concurrent / Handoff / GroupChat / Magentic / Vote）对齐 Microsoft Agent Framework 编排模型
2. **Builder→Workflow→IAgent 统一链路**：所有编排模式通过 `XXXWorkflowBuilder.build() → Workflow.as_agent() → IAgent` 收敛
3. **图驱动执行**：不可变 `WorkflowGraph` 定义拓扑，`WorkflowEngine` 按 SuperStep 模型驱动，支持循环、条件路由、并发控制
4. **引擎能力共享**：所有编排模式内部由 WorkflowEngine 驱动，自动获得检查点、重试、超时、补偿、定时器
5. **流式可观测性**：`WorkflowEvent` 流覆盖全生命周期，支持多节点独立打字机效果、工具调用卡片、Token 用量仪表
6. **事件驱动扩展**：外部事件可通过 `ExternalEvent` + `EventBus` 注入引擎，支持人工审批、消息队列等集成
7. **声明式流程定义**：`ProcessDefinition` 支持 YAML/JSON 声明式定义，通过 `compile()` 编译为引擎图
8. **业务基础设施**：SAGA 事务补偿、消息关联、审计追踪、SLA 监控、Agent 池化管理——完整的业务流程支持
9. **零侵入分层**：引擎层定义 trait 和执行原语，业务层提供 DSL 抽象和可插拔实现，通过已有 trait 交互

---

## 上一步

← [第 10 章：宏与声明式配置](../10-macros-declarative/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[扩展能力](../12-extensions/overview.md)** 以拓展 Agent 能力边界，集成网络搜索、RAG、Wiki、技能、脚本和记忆系统。

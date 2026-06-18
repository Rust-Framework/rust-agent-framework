# 第 11 章：多智能体编排

本章全面介绍 RAF 的多智能体编排系统——基于图驱动的工作流引擎，通过六种 Builder 构建专业化编排模式，统一收敛为 IAgent 门面，并提供检查点持久化、事件驱动和全链路流式可观测性。

本章适合需要构建复杂多 Agent 协作系统的架构师和高级开发者。

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

## 设计理念

1. **MAF 对齐**：六种 Builder（Sequential / Concurrent / Handoff / GroupChat / Magentic / Vote）对齐 Microsoft Agent Framework 编排模型
2. **Builder→Workflow→IAgent 统一链路**：所有编排模式通过 `XXXWorkflowBuilder.build() → Workflow.as_agent() → IAgent` 收敛
3. **图驱动执行**：不可变 `WorkflowGraph` 定义拓扑，`WorkflowEngine` 按 SuperStep 模型驱动，支持循环、条件路由、并发控制
4. **引擎能力共享**：所有编排模式内部由 WorkflowEngine 驱动，自动获得检查点、重试、超时、补偿、定时器
5. **流式可观测性**：`WorkflowEvent` 流覆盖全生命周期，支持多节点独立打字机效果、工具调用卡片、Token 用量仪表
6. **事件驱动扩展**：外部事件可通过 `ExternalEvent` + `EventBus` 注入引擎，支持人工审批、消息队列等集成

---

## 上一步

← [第 10 章：宏与声明式配置](../10-macros-declarative/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[扩展能力](../12-extensions/overview.md)** 以拓展 Agent 能力边界，集成网络搜索、RAG、Wiki、技能、脚本和记忆系统。

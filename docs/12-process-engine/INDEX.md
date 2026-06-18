# 第 12 章：业务流程编排引擎

在掌握了多智能体编排技术（第 11 章）之后，本章将深入 RAF 的业务流程编排引擎——面向企业级业务自动化场景的完整解决方案。基于第 11 章的图引擎核心，本章在其基础上构建了声明式流程定义（ProcessDefinition DSL）、八种 BPMN 风格标准活动节点、流程实例生命周期管理、增强网关与事件调度、SAGA 分布式事务补偿、Agent 团队池化管理以及消息关联审计与 SLA 监控等生产级能力。

本章适合需要在生产环境中构建可靠、可审计、可运维的业务流程自动化系统的架构师和高级开发者。

## 与多智能体编排的关系

```
多智能体编排（第 11 章）              业务流程编排（第 12 章）
─────────────────────                ─────────────────────
图引擎驱动执行                        声明式流程定义 DSL
六种编排模式                          16 种 NodeKind 节点
检查点与断点续传                       流程实例生命周期
流式可观测性                          标准活动节点
统一 IAgent 门面                      SAGA 事务补偿
                                     增强网关与定时调度
                                     Agent 团队与池化
                                     消息关联与审计
```

> **阅读提示**：本章内容建立在第 11 章的基础上，建议先掌握图引擎、SuperStep 执行模型和编排模式的基本概念。

## 章节目录

| 小节 | 标题 | 内容概要 |
|------|------|---------|
| [12.1](overview.md) | 业务流程引擎概述 | BPMN 风格流程引擎架构、与编排引擎的关系、适用场景 |
| [12.2](process-definition.md) | 流程定义与编译 | 声明式 YAML DSL、16 种 NodeKind、边条件路由、compile() 编译 |
| [12.3](standard-activities.md) | 标准活动节点 | 8 种 BPMN 风格 IExecutor：ServiceTask 到 NoneTask |
| [12.4](process-instance.md) | 流程实例与状态管理 | 6 状态生命周期、ProcessSnapshot、变量管理、引擎集成 |
| [12.5](advanced-gateways-events.md) | 增强网关、事件与定时调度 | EventBasedGateway、ComplexGateway、BoundaryEvent、TimerTrigger、CronTrigger |
| [12.6](saga-compensation.md) | SAGA 事务与补偿链 | SagaOrchestrator、BackwardRecovery、ForwardRecovery |
| [12.7](agent-team-pool.md) | Agent 团队与池化管理 | AgentTeam 能力注册、AgentPool 心跳/健康、DynamicRouter 路由 |
| [12.8](observability.md) | 消息关联、审计与 SLA | MessageCorrelation、IMessageBroker、AuditTrail、SlaTracker |

## 快速导航

- **想了解整体架构？** → [12.1 业务流程引擎概述](overview.md)
- **想用 YAML 定义业务流程？** → [12.2 流程定义 DSL](process-definition.md)
- **想使用标准活动节点？** → [12.3 标准活动节点](standard-activities.md)
- **想管理流程生命周期？** → [12.4 流程实例](process-instance.md)
- **想使用增强网关和定时调度？** → [12.5 增强网关与事件](advanced-gateways-events.md)
- **想实现分布式事务？** → [12.6 SAGA 补偿](saga-compensation.md)
- **想管理 Agent 团队？** → [12.7 Agent 团队与池化](agent-team-pool.md)
- **想集成消息中间件和审计？** → [12.8 消息关联与可观测性](observability.md)

## 核心特性

1. **声明式流程定义**：`ProcessDefinition` 支持 YAML/JSON 声明式定义，通过 `compile()` 编译为引擎图
2. **BPMN 标准兼容**：8 种标准活动节点 + 16 种 NodeKind，覆盖主流 BPMN 规范
3. **流程实例管理**：6 状态生命周期机、快照持久化、变量管理、状态守卫
4. **高级网关模式**：EventBasedGateway、ComplexGateway、BoundaryEvent、IntermediateEvent
5. **分布式事务**：SAGA 模式，支持 BackwardRecovery 和 ForwardRecovery 两种恢复策略
6. **Agent 资源管理**：AgentTeam 角色注册、AgentPool 连接池与健康检查、DynamicRouter 能力路由
7. **企业级可观测性**：消息关联匹配、审计追踪、SLA 监控、指标采集
8. **消息中间件集成**：`IMessageBroker` 抽象，支持 Kafka、RabbitMQ、Redis 等

---

## 上一步

← [第 11 章：多智能体编排技术](../11-multi-agent/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[第 13 章：扩展能力](../13-extensions/overview.md)** 以拓展 Agent 能力边界，集成网络搜索、RAG、Wiki、技能、脚本和记忆系统。

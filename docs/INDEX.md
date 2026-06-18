# 目录索引

## 第 1 章：快速入门

| 小节 | 标题 |
|------|------|
| [1.1](01-quick-start/installation.md) | 环境安装与项目创建 |
| [1.2](01-quick-start/first-agent.md) | 你的第一个 Agent |
| [1.3](01-quick-start/core-concepts.md) | 核心概念概览 |
| [1.4](01-quick-start/builtin-tools-intro.md) | 内置工具快速体验 |

## 第 2 章：核心架构

| 小节 | 标题 |
|------|------|
| [2.1](02-core-architecture/layered-design.md) | 框架分层设计 |
| [2.2](02-core-architecture/type-system.md) | 核心类型系统 |
| [2.3](02-core-architecture/message-model.md) | 消息与流式模型 |
| [2.4](02-core-architecture/error-handling.md) | 错误处理体系 |
| [2.5](02-core-architecture/crate-map.md) | Workspace 与 Crate 地图 |

## 第 3 章：Agent 引擎

| 小节 | 标题 |
|------|------|
| [3.1](03-agent-engine/chat-client-agent.md) | ChatClientAgent 详解 |
| [3.2](03-agent-engine/agent-builder.md) | AgentBuilder 流式构建器 |
| [3.3](03-agent-engine/run-lifecycle.md) | Agent 运行生命周期（三阶段） |
| [3.4](03-agent-engine/streaming.md) | 流式输出处理 |
| [3.5](03-agent-engine/compression-strategies.md) | 上下文压缩策略 |

## 第 4 章：工具系统

| 小节 | 标题 |
|------|------|
| [4.1](04-tool-system/itool-trait.md) | ITool trait 与 ToolResult |
| [4.2](04-tool-system/tool-registry.md) | ToolRegistry 工具注册表 |
| [4.3](04-tool-system/approval-required-tool.md) | ApprovalRequiredTool 审批包装 |
| [4.4](04-tool-system/builtin-filesystem-tools.md) | 内置文件系统工具 |
| [4.5](04-tool-system/run-command-tool.md) | RunCommand 命令执行工具 |
| [4.6](04-tool-system/custom-tools.md) | 自定义工具开发指南 |
| [4.7](04-tool-system/scope-tool.md) | IScopeTool 工作区感知 |

## 第 5 章：上下文提供器

| 小节 | 标题 |
|------|------|
| [5.1](05-context-providers/overview.md) | IContextProvider 概述 |
| [5.2](05-context-providers/history-provider.md) | InMemoryHistoryProvider 历史管理 |
| [5.3](05-context-providers/skills-provider.md) | AgentSkillsProvider 技能注入 |
| [5.4](05-context-providers/custom-provider.md) | 自定义上下文提供器 |

## 第 6 章：会话管理

| 小节 | 标题 |
|------|------|
| [6.1](06-sessions/isession.md) | ISession 会话接口 |
| [6.2](06-sessions/agent-session.md) | AgentSession 默认实现 |
| [6.3](06-sessions/session-stores.md) | 会话存储（内存/文件/隔离） |
| [6.4](06-sessions/provider-state.md) | ProviderState 状态持久化 |

## 第 7 章：人机协同与审批

| 小节 | 标题 |
|------|------|
| [7.1](07-hitl-approval/hitl-overview.md) | 人机协同（HITL）概述 |
| [7.2](07-hitl-approval/approval-flow.md) | 审批流完整链路 |
| [7.3](07-hitl-approval/tool-approval-api.md) | ToolApprovalRequest/Response API |
| [7.4](07-hitl-approval/resume-cancel.md) | 中断恢复与取消机制 |

## 第 8 章：工作区管理

| 小节 | 标题 |
|------|------|
| [8.1](08-workspace-management/scope-overview.md) | WorkspaceScope 工作区范围 |
| [8.2](08-workspace-management/workspace-context-provider.md) | WorkspaceContextProvider |
| [8.3](08-workspace-management/path-guard.md) | 路径守卫与跨范围检测 |
| [8.4](08-workspace-management/cross-scope-approval.md) | 跨范围审批集成 |

## 第 9 章：ChatClient 管道

| 小节 | 标题 |
|------|------|
| [9.1](09-chat-client-pipeline/decorator-pattern.md) | 装饰器模式与管道架构 |
| [9.2](09-chat-client-pipeline/function-invoking.md) | FunctionInvokingChatClient 工具调用循环 |
| [9.3](09-chat-client-pipeline/llm-providers.md) | LLM 提供商（OpenAI / DeepSeek） |
| [9.4](09-chat-client-pipeline/stream-processing.md) | 流式处理与中间件 |

## 第 10 章：宏与声明式配置

| 小节 | 标题 |
|------|------|
| [10.1](10-macros-declarative/tool-macro.md) | #[tool] 属性宏详解 |
| [10.2](10-macros-declarative/macro-type-mapping.md) | Rust 类型到 JSON Schema 映射 |
| [10.3](10-macros-declarative/declarative-config.md) | 声明式 Agent/Workflow 配置 |
| [10.4](10-macros-declarative/agent-schema.md) | AgentSchema v1.0 规范 |

## 第 11 章：多智能体编排

| 小节 | 标题 |
|------|------|
| [11.1](11-multi-agent/overview.md) | 编排引擎概述 |
| [11.2](11-multi-agent/sequential-workflow.md) | SequentialWorkflow 顺序编排 |
| [11.3](11-multi-agent/concurrent-workflow.md) | ConcurrentWorkflow 并发编排 |
| [11.4](11-multi-agent/handoff-workflow.md) | HandoffWorkflow 交接编排 |
| [11.5](11-multi-agent/custom-orchestrations.md) | 自定义编排（WorkflowBuilder） |
| [11.6](11-multi-agent/checkpoints.md) | 检查点与断点续传 |
| [11.7](11-multi-agent/workflow-as-agent.md) | IAgent 统一门面 |
| [11.8](11-multi-agent/group-chat-workflow.md) | GroupChatWorkflow 群聊编排 |
| [11.9](11-multi-agent/magentic-workflow.md) | MagenticWorkflow 自主编排 |
| [11.10](11-multi-agent/vote-workflow.md) | VoteWorkflow 投票聚合 |

## 第 12 章：扩展能力

| 小节 | 标题 |
|------|------|
| [12.1](12-extensions/overview.md) | 扩展体系概述 |
| [12.2](12-extensions/websearch.md) | 网络搜索（WebSearch / WebFetch） |
| [12.3](12-extensions/rag.md) | 检索增强生成（RAG） |
| [12.4](12-extensions/wiki.md) | Wiki 知识引擎 |
| [12.5](12-extensions/skills.md) | Agent 技能系统 |
| [12.6](12-extensions/rhai-scripts.md) | Rhai 脚本引擎 |
| [12.7](12-extensions/memory.md) | SkillMemory 记忆系统 |

## 第 13 章：宿主服务

| 小节 | 标题 |
|------|------|
| [13.1](13-host-service/overview.md) | Host Service 概述 |
| [13.2](13-host-service/acp-protocol.md) | ACP 协议与消息格式 |
| [13.3](13-host-service/transports.md) | 传输层（Stdio / WebSocket） |
| [13.4](13-host-service/agent-registry.md) | Agent 注册与发现 |
| [13.5](13-host-service/multi-agent-orchestration.md) | 三层多智能体编排模型 |
| [13.6](13-host-service/tagged-streaming.md) | 标签化流式输出 |

## 第 14 章：附录

| 小节 | 标题 |
|------|------|
| [14.1](14-appendix/api-reference.md) | API 速查表 |
| [14.2](14-appendix/crate-dependency-graph.md) | Crate 依赖关系图 |
| [14.3](14-appendix/glossary.md) | 术语表 |
| [14.4](14-appendix/faq.md) | 常见问题 |
| [14.5](14-appendix/migration-from-maf.md) | 从 MAF 迁移指南 |
| [14.6](14-appendix/performance-tuning.md) | 性能调优指南 |

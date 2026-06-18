# 14.3 术语表

## A

### Agent（智能体）

RAF 中的核心概念，指能够使用 LLM、工具和其他 Agent 进行推理、规划和执行的自主软件组件。通过 `IAgent` trait 定义统一接口。

### AgentBuilder

流式构建器，用于以声明方式构建 Agent 实例。支持链式调用配置 ChatClient、工具、上下文提供器和指令。

### AgentRegistry

Agent 注册表，管理宿主服务中所有可用 Agent 的注册、查找和子 Agent 发现。

### AgentSchema

声明式 Agent 定义规范（v1.0），与 Microsoft Agent Framework 兼容。支持 JSON/YAML/TOML 三种格式。

### AgentSession

默认的 `ISession` 实现，使用内存存储管理对话历史和 ProviderState。

### ACP（Agent Client Protocol）

Agent 客户端协议，基于 JSON-RPC 2.0，定义客户端与 Agent 宿主之间的通信规范。

### ApprovalRequiredTool

包装任意 ITool，标记为需要人工审批才能执行。`FunctionInvokingChatClient` 在执行前检查 `requires_approval()`。

## C

### ChatClient

LLM 客户端抽象（`IChatClient` trait），封装与 LLM 提供商的通信。支持 OpenAI 和 DeepSeek 兼容的 API。

### Chunker

RAG 管道中的文档分块组件。支持递归分块和语义分块策略。

### Checkpoint（检查点）

工作流执行状态的完整快照，支持增量 + 全量压缩策略。用于断点续传和故障恢复。

### Condition（条件）

工作流图中的条件路由组件。支持 `ExpressionCondition`（闭包）、`VariableCondition`（变量比较）和 `VariableEdgeCondition`（边变量比较）。

### ContextResult

上下文注入载体，Provider 在 Pre-invocation 阶段返回的上下文增强内容，包含 instructions、messages 和 tools。

### ContextProvider（上下文提供器）

Agent 调用生命周期的核心扩展点（`IContextProvider` trait）。在每次调用前后注入上下文。

## E

### Edge（边）

工作流图中的有向边，定义节点间的数据流向和条件路由。

### Executor（执行器）

工作流图中节点的运行时行为抽象（`IExecutor` trait）。内置类型包括 AgentExecutor、FunctionExecutor、RhaiExecutor 等。

## F

### FunctionInvokingChatClient

工具调用循环装饰器，包装 IChatClient，自动处理 LLM 的 function calling 响应并执行对应的 ITool。

## H

### HandoffWorkflow

交接编排模式——分类 Agent 分析请求后路由给最合适的专业 Agent。类似 OpenAI Swarm 的 Handoff 概念。

### HumanTaskExecutor

人工任务执行器，用于等待人工输入或审批的工作流节点。

## I

### IAgent

Agent 核心接口，定义 Agent 的标识、元数据、运行、子 Agent 发现和生命周期管理。

### IChatClient

LLM 聊天客户端接口，封装模型选择、消息发送和流式响应处理。

### IContextProvider

上下文提供器接口，在 Agent 调用前后注入上下文。

### IExecutor

工作流节点执行器接口，定义节点的消息处理逻辑。

### IRetriever

RAG 检索器接口，根据查询文本检索相关的文档块。

### ISession

会话接口，管理对话历史、ProviderState 和生命周期。

### ITool

工具接口，定义工具的名称、描述、参数 Schema 和执行逻辑。

## M

### MAF（Microsoft Agent Framework）

微软 AI Agent 框架，RAF 的设计参考和兼容目标。

### MemoryAgent

后台记忆整合 Agent，由 SkillMemoryContextProvider 触发，定期整合对话记忆。

### MemoryConsolidationWorker

后台工作线程，负责任务合并和串行化记忆整合作业。

## N

### Node（节点）

工作流图中的执行单元，每个节点绑定一个 IExecutor。

## P

### Port（端口）

工作流图的外部请求入口，允许从外部向图内部节点注入消息。

### PromptAgentData

声明式提示词 Agent 的数据结构，包含模型配置、工具列表、模板和指令。

## R

### RhaiExecutor

Rhai 脚本作为工作流节点执行器，实现 IExecutor trait。

### RhaiRuntime

Rhai 脚本运行时环境，提供沙箱化执行、变量注入和 JSON 转换。

### RhaiTool

Rhai 脚本作为 Agent 工具，实现 ITool trait。

## S

### SequentialWorkflow

顺序编排模式——Agent 按顺序执行，每个后续 Agent 接收前一个的输出。

### SessionBridge

ACP 会话与 RAF Agent 会话之间的桥梁，管理会话映射、取消令牌和目标 Agent。

### SkillMemory

持久化跨会话记忆系统，通过后台整合保持 Agent 记忆的连续性。

### SubAgentStatusTracker

子 Agent 执行状态追踪器，为标签化流式输出提供状态元数据。

### SubFlowExecutor

嵌套子工作流执行器，将完整的工作流图作为节点执行。

### SuperStep

工作流引擎的批量同步执行模型（类似 Google Pregel），每个 SuperStep 并行执行当前活跃节点。

## T

### Tagged Streaming

标签化流式输出，每个 `session/update` 携带 `_meta.raf.agent_id` 标签，使前端能区分多 Agent 输出。

### ToolApprovalRequest / ToolApprovalResponse

工具审批的请求和响应类型，用于人机协同（HITL）流程。

### ToolRegistry

工具注册表，管理工具注册和按名称查找。

### ToolResolver

工具解析器，将声明式工具定义（ToolDecl）解析为运行时的 ITool 实例。

### ToolResult

工具执行结果的统一返回类型，包含成功/失败标志、数据和错误信息。

## W

### WorkflowAsAgent

工作流适配器，将任意编排模式包装为 IAgent，实现"编排即 Agent"的设计哲学。

### WorkflowBuilder

工作流构建器，通过编程方式定义自定义的有向图拓扑。

### WorkflowGraph

不可变的工作流图定义，通过 WorkflowBuilder 构建后冻结，是工作流执行的数据源。

### WorkflowAgent

WorkflowBuilder 构建的图直接包装为 IAgent 的类型。

### WorkspaceScope

工作区范围定义，限制 Agent 工具的文件系统访问范围。

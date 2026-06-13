# rust-agent-core

核心接口与类型定义层，是整个框架的基石。

## 功能定位

定义所有核心 trait 和基础数据类型，为上层 crate 提供统一的抽象契约。

- **接口定义**: `IAgent`, `IChatClient`, `ITool`, `IMiddleware`, `ISession`, `IWorkflow`
- **核心类型**: `ChatMessage`, `AgentResponse`, `AgentStreamChunk`, `ChatStreamChunk`, `AgentId`, `ToolCall`, `ToolResult`
- **流式抽象**: `BoxStream<T>` 类型别名、`collect_agent_response()` 流聚合工具
- **基础实现**: `AgentSession`（内存会话）、`AIFunction`（函数式工具）、`ToolRegistry`（工具注册表）
- **错误体系**: `AgentError` 统一错误枚举

## 专属职责

- 定义框架级 trait 接口（I 前缀命名，致敬 MAF）
- 定义跨 crate 共享的数据结构
- 提供流式基础设施（`BoxStream`, `collect_agent_response`）
- 提供 session 和 tool 的默认实现

## 不做什么

- 不依赖任何具体的 LLM provider 或 HTTP 客户端
- 不实现 `IAgent` 的具体编排逻辑（由 `rust-agent-framework` 负责）
- 不实现 `IChatClient` 的具体通信逻辑（由 `rust-agent-client` 负责）
- 不实现 `IWorkflow` 的具体编排逻辑（由 `rust-agent-workflow` 负责）
- 不提供持久化存储后端（可由外部扩展 `ISession` 实现）

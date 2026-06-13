# rust-agent-framework

Agent 编排与运行时层，框架的核心引擎。

## 功能定位

实现 `IAgent` trait 并提供 agent 运行时，将 `IChatClient`、`ITool`、`IMiddleware` 组装为可执行的 agent。

- **ChatClientAgent**: `IAgent` 的主要实现，遵循 MAF 的 `ChatClientAgent` 模式
  - 组合 chat client + instructions + tools + middleware
  - 管理对话历史
  - 将 `ChatStreamChunk` 映射为 `AgentStreamChunk`
- **AgentRuntime**: agent 运行时宿主
  - agent 注册与查找
  - 消息路由到指定 agent

## 专属职责

- 实现 `IAgent::stream()` 的完整生命周期：middleware 拦截 → 消息组装 → chat client 调用 → 流映射 → 历史记录
- 管理 agent 的注册、发现和消息分发
- 提供 builder 模式配置 agent（instructions、tools、middleware、description）

## 不做什么

- 不实现 `IChatClient`（由 `rust-agent-client` 负责）
- 不实现 workflow 编排（由 `rust-agent-workflow` 负责）
- 不做 tool 的自动调用循环（tool call loop 需上层或用户自行编排）
- 不提供 CLI 或 REPL 交互

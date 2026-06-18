# 第 2 章：核心架构

本章深入解析 Rust Agent Framework 的架构设计——分层模型、类型系统、消息模型、错误处理机制和 crate 地图。

## 章节导航

1. **[分层设计](./layered-design.md)** — 四层架构：核心抽象层 → LLM 客户端层 → 框架运行时层 → 扩展层。理解 RAF 的"洋葱模型"设计哲学。
2. **[类型系统](./type-system.md)** — 核心类型详解：`AgentId`、`AgentMetadata`、`FinishReason`、`ResponseMetadata`、`ToolCall`、`Usage`。它们在框架中的角色和数据流。
3. **[消息模型](./message-model.md)** — `ChatMessage` 结构、`Content` 枚举（12 个变体）、`MessageRole` 四角色、流式 `AgentResponseUpdate` 与 `AgentResponseResult` 的完整管道。
4. **[错误处理](./error-handling.md)** — `AgentError` 枚举（8 个变体）、`Result` 类型别名、`ToolResult` 结构体（ok/data/error）、错误在框架各层的传播路径。
5. **[Crate 地图](./crate-map.md)** — 全部 15 个 crate 的职责划分、依赖关系图和分类（核心/扩展）。帮助你按需选择依赖。

## 阅读建议

- **架构章节（分层设计、Crate 地图）**：适合所有开发者，帮助你在使用框架前建立全局认知。
- **类型与消息（类型系统、消息模型）**：适合需要自定义 Agent、编写 ContextProvider 或扩展框架的高级开发者。
- **错误处理**：在调试框架级问题时查阅，理解错误如何从 LLM 传输层传播到用户代码。

## 架构哲学

RAF 遵循 **MAF（Microsoft Agent Framework）兼容设计**，核心思想：

1. **接口分离**：业务逻辑只依赖 trait（`IAgent`、`IChatClient`、`ITool`），不依赖具体实现。
2. **管道装饰器**：通过 `IChatClient` 包装链实现关注点分离——工具调用、会话持久化、缓存等各司其职。
3. **分层扩展**：核心层保持轻量（仅特质和类型），运行时层提供通用能力，扩展层按需加载。
4. **流式优先**：所有 LLM 调用仅支持流式（`BoxStream`），从设计上杜绝阻塞等待。

---

## 上一步

← [第 1 章：快速入门](../01-quick-start/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[Agent 引擎](../03-agent-engine/chat-client-agent.md)** 以深入 Agent 运行时引擎，理解 ChatClientAgent 的构建、运行生命周期和流式管道。

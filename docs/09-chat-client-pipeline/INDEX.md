# 第 9 章：ChatClient 管道

ChatClient 管道是 RAF 的核心架构创新之一。通过将 LLM 客户端抽象为 `IChatClient` trait，并用**装饰器模式**构建可组合的处理管道，RAF 实现了工具调用循环、审批流程、会话持久化等横切关注点的解耦。本章深入管道架构的设计理念、`FunctionInvokingChatClient` 的状态机实现、LLM 提供商的接入方式，以及流式处理链路。

| 小节 | 标题 |
|------|------|
| [9.1](decorator-pattern.md) | 装饰器模式与管道架构 |
| [9.2](function-invoking.md) | FunctionInvokingChatClient 工具调用循环 |
| [9.3](llm-providers.md) | LLM 提供商（OpenAI / DeepSeek） |
| [9.4](stream-processing.md) | 流式处理与中间件 |

## 快速导航

- **为什么选用装饰器模式**：参见 [9.1 — 装饰器模式](decorator-pattern.md)。
- **工具调用循环的完整实现**：参见 [9.2 — FunctionInvokingChatClient](function-invoking.md)。
- **如何接入新的 LLM 提供商**：参见 [9.3 — LLM 提供商](llm-providers.md)。
- **从 SSE 字节到 AgentResponseResult 的完整链路**：参见 [9.4 — 流式处理](stream-processing.md)。

## 管道架构总览

```mermaid
graph TB
    subgraph "ChatClientAgent"
        A[组装消息<br/>Provider 调用<br/>压缩]
    end

    subgraph "ChatClient 管道"
        B[FunctionInvokingChatClient<br/>工具调用循环 + 审批]
        C[PerServiceCallPersistingChatClient<br/>会话持久化]
        D[DeepSeekChatClient / OpenAiChatClient<br/>供应商特定客户端]
        E[ChatClient<br/>HTTP/SSE 传输引擎]
    end

    A -->|IChatClient::run()| B
    B --> C
    C --> D
    D --> E
    E -->|HTTP POST| LLM[LLM API]
    LLM -->|SSE| E
    E -->|AgentResponseUpdate| D
    D --> C
    C --> B
    B --> A

    style B fill:#4CAF50,color:white
    style E fill:#2196F3,color:white
```

## 核心类型速览

| 类型 | 所在 Crate | 用途 |
|------|-----------|------|
| `IChatClient` | `rust_agent_core::chat_client` | 聊天客户端 trait，管道叶子接口 |
| `DelegatingChatClient` | `rust_agent_core::chat_client` | 装饰器基类，透传所有方法 |
| `ChatClientBuilder` | `rust_agent_core::chat_client` | 管道构建器，按序组装装饰器 |
| `FunctionInvokingChatClient` | `rust_agent_framework::chat_client_decorators` | 工具调用循环装饰器 |
| `ChatClient` | `rust_agent_client::chat_client` | 通用 HTTP/SSE 传输引擎 |
| `DeepSeekChatClient` | `rust_agent_client::deepseek_client` | DeepSeek 提供商客户端 |
| `OpenAiChatClient` | `rust_agent_client::openai_client` | OpenAI 提供商客户端 |
| `ChatClientOptions` | `rust_agent_client::options` | 客户端配置（api_base, api_key, model...） |
| `AgentResponseConverter` | `rust_agent_framework::converter` | SSE 事件 → 公共 API 转换器 |
| `SseStream` | `rust_agent_client::transport` | SSE 字节流解析器 |

---

## 上一步

← [第 8 章：工作区管理](../08-workspace-management/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[宏与声明式配置](../10-macros-declarative/tool-macro.md)** 以学习声明式编程能力，使用 `#[tool]` 宏和 YAML/JSON 配置减少样板代码。

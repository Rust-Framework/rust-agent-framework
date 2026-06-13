# rust-agent-client

LLM provider 客户端实现层，遵循 MAF 的 provider-leading 命名规范。

## 功能定位

实现 `IChatClient` trait，封装与具体 LLM API 的通信细节，向上层暴露纯流式接口。

- **OpenAIChatClient**: OpenAI API 的 `IChatClient` 实现（provider-leading 命名，致敬 MAF ADR-0021）
- **ChatClientOptions**: 客户端配置（API base、key、model、temperature 等）

## 专属职责

- 实现 `IChatClient::stream()` 方法，将 LLM API 的 SSE/流式响应转换为 `BoxStream<Result<ChatStreamChunk>>`
- 管理 API 认证、请求构建、响应解析
- 每个 provider 一个实现，命名以 provider 名开头（如 `OpenAI`ChatClient）

## 不做什么

- 不做 agent 编排或消息路由
- 不做 tool 调用循环
- 不做会话管理
- 不做 prompt 模板渲染
- 不定义新的 trait 或接口（接口在 `rust-agent-core` 中定义）

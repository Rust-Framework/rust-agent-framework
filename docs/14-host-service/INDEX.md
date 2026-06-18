# 第 14 章：宿主服务

本章详细介绍 RAF 的宿主服务层——基于 ACP（Agent Client Protocol）协议的多 Agent 托管服务器，支持 Stdio 和 WebSocket 双传输模式，提供 Agent 注册发现、多层编排和标签化流式输出能力。

本章面向需要将 RAF Agent 部署为可远程访问服务的运维和平台工程师。

## 章节目录

| 小节 | 标题 | 内容概要 |
|------|------|---------|
| [14.1](overview.md) | Host Service 概述 | ACP 服务器架构、双传输、多 Agent 托管 |
| [14.2](acp-protocol.md) | ACP 协议与消息格式 | 消息格式、请求/响应流、AgentRunOptions 映射 |
| [14.3](transports.md) | 传输层 | Stdio 传输、WebSocket 传输、配置切换 |
| [14.4](agent-registry.md) | Agent 注册与发现 | AgentRegistry、内置 Agent 工厂、声明式加载 |
| [14.5](multi-agent-orchestration.md) | 三层多智能体编排模型 | SessionBridge、ACP↔RAF 会话映射 |
| [14.6](tagged-streaming.md) | 标签化流式输出 | SubAgentStatusTracker、source 标签、前端渲染 |

## 快速导航

- **想了解整体架构？** → [14.1 概述](overview.md)
- **想了解 ACP 协议细节？** → [14.2 ACP 协议](acp-protocol.md)
- **想配置传输方式？** → [14.3 传输层](transports.md)
- **想注册多个 Agent？** → [14.4 Agent 注册](agent-registry.md)
- **想理解编排模型？** → [14.5 三层编排模型](multi-agent-orchestration.md)
- **想实现多 Agent 流式渲染？** → [14.6 标签化流式输出](tagged-streaming.md)

---

## 上一步

← [第 13 章：扩展能力](../13-extensions/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[第 15 章：附录](../15-appendix/api-reference.md)** 以查阅 API 速查表、术语表、常见问题解答和性能调优建议。

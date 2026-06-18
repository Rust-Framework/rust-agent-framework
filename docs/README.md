# Rust Agent Framework 开发者指南

**Rust Agent Framework (RAF)** 是一个面向 AI 智能体开发者的企业级框架，对标 Microsoft Agent Framework (MAF) 设计理念，以 Rust 语言实现的异步、流式、多智能体编排平台。

> 📖 [**阅读前言**](FOREWORD.md) — 了解本书的编写背景、目标读者和阅读建议

## 本书定位

本书是一本面向 AI 智能体开发者的完整技术手册。无论你是正在构建第一个简单 Agent，还是设计复杂的多智能体编排系统，你都可以在本书中找到详尽的 API 参考、架构设计原理和最佳实践。

## 框架版本

- **版本**: 0.1.0
- **Rust 版本**: 2021 edition
- **许可协议**: MIT

## 章节概览

| 章节 | 标题 | 内容 |
|------|------|------|
| 第 1 章 | [快速入门](01-quick-start/) | 环境搭建、第一个 Agent、核心概念 |
| 第 2 章 | [核心架构](02-core-architecture/) | 框架分层、类型系统、消息模型 |
| 第 3 章 | [Agent 引擎](03-agent-engine/) | ChatClientAgent、AgentBuilder、运行生命周期 |
| 第 4 章 | [工具系统](04-tool-system/) | ITool、ToolRegistry、内置工具、自定义工具 |
| 第 5 章 | [上下文提供器](05-context-providers/) | IContextProvider、历史管理、技能注入 |
| 第 6 章 | [会话管理](06-sessions/) | ISession、会话存储、TTL 管理 |
| 第 7 章 | [人机协同与审批](07-hitl-approval/) | ApprovalRequiredTool、审批流、中断恢复 |
| 第 8 章 | [工作区管理](08-workspace-management/) | WorkspaceScope、路径守卫、跨范围审批 |
| 第 9 章 | [ChatClient 管道](09-chat-client-pipeline/) | 装饰器模式、工具调用循环、LLM 提供商 |
| 第 10 章 | [宏与声明式配置](10-macros-declarative/) | #[tool] 宏、JSON/YAML/TOML 配置 |
| 第 11 章 | [多智能体编排](11-multi-agent/) | Builder 体系、六种编排模式、引擎化执行、IAgent 统一门面、检查点 |
| 第 12 章 | [扩展能力](12-extensions/) | 网络搜索、RAG、Wiki、技能系统、Rhai 脚本 |
| 第 13 章 | [宿主服务](13-host-service/) | ACP 协议、Stdio/WebSocket 传输、智能体注册 |
| 第 14 章 | [附录](14-appendix/) | Crate 地图、API 速查、术语表、常见问题 |

## 快速导航

- [完整目录索引](INDEX.md)
- [第 1 章：快速入门](01-quick-start/)
- [API 速查表](14-appendix/api-reference.md)

## 阅读建议

- **首次使用 RAF**: 建议从第 1 章开始，按顺序阅读前 4 章
- **已有 Agent 开发经验**: 可直接跳到第 4 章（工具系统）或第 9 章（ChatClient 管道）
- **构建生产系统**: 建议完整阅读第 7 章（审批）、第 8 章（工作区管理）、第 13 章（宿主服务）
- **设计复杂工作流**: 重点阅读第 11 章（多智能体编排）

---

> 本书内容基于 RAF v0.1.0 源码深度分析编写，力求准确反映框架的设计精髓和最佳实践。

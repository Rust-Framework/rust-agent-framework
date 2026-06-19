# 第 10 章：宏与声明式配置

本章涵盖 RAF 框架的两种减少样板代码的方式：

1. **`#[tool]` 属性宏**（10.1-10.2）：编译期自动生成 `ITool` trait 实现、JSON Schema 和参数反序列化器。适合定义工具的 Rust 开发者。

2. **声明式配置系统**（10.3-10.6）：通过 `DeclAgentBuilder` 加载 YAML / JSON / TOML 文件来定义完整的 Agent，无需硬编码模型、工具和上下文提供器。适合部署和团队协作场景。

> **术语说明**：本章标题中"声明式"指基于外部配置文件的 Agent 定义方式，与 Rust 宏（`#[tool]`）是两个独立的概念。`DeclAgentBuilder` 是声明式配置的 Rust 入口，接受 YAML 文件并生成 `Arc<dyn IAgent>`，与 `AgentBuilder` 互为替代方案。

本章面向希望减少样板代码、使用声明式配置快速构建 Agent 系统的开发者。

## 章节目录

| 小节 | 标题 | 内容概要 |
|------|------|---------|
| [10.1](tool-macro.md) | `#[tool]` 属性宏详解 | 异步函数模式、impl 块模式、结构体模式，自动生成 ITool 实现、参数反序列化器、JSON Schema、kind 分类
| [10.2](macro-type-mapping.md) | Rust 类型到 JSON Schema 映射 | 完整类型映射表，`#[param]` 属性，`Option<T>` 检测，数组与嵌套泛型 |
| [10.3](declarative-config.md) | 声明式 Agent/Workflow 配置 | `rust-agent-decl` 核心类型，多格式支持，ToolResolver 工具解析，kind 字段溯源，便捷函数 |
| [10.4](agent-schema.md) | AgentSchema v1.0 规范 | Agent 类型体系，模型配置，工具绑定，连接定义，模板引擎，完整示例 |
| [10.5](config-reference.md) | 声明式配置完整字段参考 | 逐字段说明、类型/必填/默认值、全部 kind 的可用值、YAML/JSON/TOML 三格式完整示例 |
| [10.6](declarative-tutorial.md) | 声明式 Agent 配置实战教程 | 从零到一构建声明式 Agent，涵盖工具/工作区/记忆/技能，调试技巧，AgentBuilder 迁移对照 |
| [10.7](migration-guide.md) | AgentBuilder → DeclAgentBuilder 迁移指南 | 详细对照：模型/工具/工作区/上下文提供器的逐项迁移，混合模式，常见陷阱 |
| [10.8](integration-patterns.md) | 组件联动规则 | workspace+tools 自动路由、IScopeTool 检测、Skills 工具注入、Provider 顺序、name-expansion 等 7 项联动规则 |

## 快速导航

- **想快速定义工具？** → [10.1 `#[tool]` 属性宏详解](tool-macro.md)
- **想了解类型如何映射为 JSON Schema？** → [10.2 类型映射参考](macro-type-mapping.md)
- **想用 YAML 文件声明 Agent？** → [10.3 声明式配置](declarative-config.md)
- **想了解 AgentSchema 规范全貌？** → [10.4 AgentSchema v1.0 规范](agent-schema.md)
- **查阅全部可配置字段及有效值？** → [10.5 配置字段完全参考](config-reference.md)
- **手把手用 YAML 构建生产级 Agent？** → [10.6 声明式 Agent 实战教程](declarative-tutorial.md)
- **从 AgentBuilder 代码迁移到声明式？** → [10.7 迁移指南](migration-guide.md)
- **理解 workspace+tools 等组件的联动规则？** → [10.8 组件联动规则](integration-patterns.md)

## 设计理念

RAF 的宏声明式系统遵循以下原则：

1. **编译期安全**：`#[tool]` 宏在编译期完成所有类型分析和代码生成，零运行时反射开销
2. **MAF 兼容**：AgentSchema v1.0 与 Microsoft Agent Framework 格式完全兼容，可直接解析 MAF YAML 文件
3. **渐进式采用**：可以先用 `#[tool]` 宏写工具、用 `AgentBuilder` 构建 Agent，再迁移到 `DeclAgentBuilder` + 声明式配置文件，三者可混合使用
4. **零样板代码**：通过属性宏自动生成 ITool 实现；通过声明式配置避免在代码中硬编码模型和工具列表

---

## 上一步

← [第 9 章：ChatClient 管道](../09-chat-client-pipeline/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[多智能体编排](../11-multi-agent/overview.md)** 以探索多智能体编排系统，掌握图驱动工作流引擎和六种编排模式。

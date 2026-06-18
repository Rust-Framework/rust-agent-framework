# 第 10 章：宏与声明式配置

本章深入介绍 RAF 框架的声明式编程能力——包括 `#[tool]` 属性宏的自动代码生成机制、Rust 类型到 JSON Schema 的映射规则，以及基于 JSON/YAML/TOML 的声明式 Agent 和 Workflow 配置系统。

本章面向希望减少样板代码、使用声明式配置快速构建 Agent 系统的开发者。

## 章节目录

| 小节 | 标题 | 内容概要 |
|------|------|---------|
| [10.1](tool-macro.md) | `#[tool]` 属性宏详解 | 异步函数模式与结构体模式，自动生成 ITool 实现、参数反序列化器、JSON Schema |
| [10.2](macro-type-mapping.md) | Rust 类型到 JSON Schema 映射 | 完整类型映射表，`#[param]` 属性，`Option<T>` 检测，数组与嵌套泛型 |
| [10.3](declarative-config.md) | 声明式 Agent/Workflow 配置 | `rust-agent-decl` 核心类型，多格式支持，ToolResolver 工具解析，便捷函数 |
| [10.4](agent-schema.md) | AgentSchema v1.0 规范 | Agent 类型体系，模型配置，工具绑定，连接定义，模板引擎，完整示例 |

## 快速导航

- **想快速定义工具？** → [10.1 `#[tool]` 属性宏详解](tool-macro.md)
- **想了解类型如何映射为 JSON Schema？** → [10.2 类型映射参考](macro-type-mapping.md)
- **想用 YAML 文件声明 Agent？** → [10.3 声明式配置](declarative-config.md)
- **想了解 AgentSchema 规范全貌？** → [10.4 AgentSchema v1.0 规范](agent-schema.md)

## 设计理念

RAF 的声明式系统遵循以下原则：

1. **编译期安全**：`#[tool]` 宏在编译期完成所有类型分析和代码生成，零运行时反射开销
2. **MAF 兼容**：AgentSchema v1.0 与 Microsoft Agent Framework 格式完全兼容，可直接解析 MAF YAML 文件
3. **渐进式采用**：可以先用宏写工具，再迁移到声明式配置文件，两者可混合使用
4. **零样板代码**：通过属性宏自动生成 Schema、反序列化器、ITool 实现，开发者只需关注业务逻辑

---

## 上一步

← [第 9 章：ChatClient 管道](../09-chat-client-pipeline/INDEX.md)

## 下一步

阅读完本章后，建议继续阅读 **[多智能体编排](../11-multi-agent/overview.md)** 以探索多智能体编排系统，掌握图驱动工作流引擎和六种编排模式。

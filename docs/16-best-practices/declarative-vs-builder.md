# 声明式 vs AgentBuilder

> **本节目标**：在 `AgentSchema` v1.0 声明式配置（JSON / YAML / TOML）与程序化 `AgentBuilder` 之间做出合理取舍。

## 1. 目标

读完本节，你应能：判断「这个 Agent 用声明式配置还是 `AgentBuilder` 更合适」，并知道两者何时可以混用、如何切换。

## 2. 核心概念

**声明式配置（AgentSchema v1.0）**：用 JSON / YAML / TOML 描述 Agent 与其 Workflow。好处是可读、可版本化、可离线校验、变更无需重新编译。适合**配置驱动的场景**：交给非 Rust 开发者编辑、按环境切换、模板化产出。

**AgentBuilder**：程序化 JSON 在代码里链式构建。好处是类型安全、可编译期校验、能与 Rust 逻辑动态交互（如条件分支注册工具）。适合**代码驱动的场景**：构建逻辑跟随代码分支、需要与业务逻辑强耦合。

核心区别一句话：**声明式把配置当数据，AgentBuilder 把配置当代码。**

## 3. 可运行示例代码片段

**（一）声明式（TOML）定义 Agent：**

```toml
# agent.toml —— AgentSchema v1.0
[schema]
version = "1.0"

[agent]
id = "support_bot"
instructions = "你只处理售后问题。"
```

```rust
use rust_agent_framework::declarative::AgentSchema;
use std::fs;

let toml = fs::read_to_string("agent.toml")?;
let schema: AgentSchema = toml_rust::from_str(&toml)?;   // 解析声明式配置
let agent = schema.build()?;                              // 编译为 IAgent
```

**（二）程序化 `AgentBuilder`（代码驱动）：**

```rust
use rust_agent_framework::{AgentBuilder, ToolRegistry};

#[rust_agent_framework::tool(description = "读取用户等级")]
async fn read_tier(user_id: u64) -> rust_agent_framework::ToolResult {
    rust_agent_framework::ToolResult::success(serde_json::json!({ "tier": "gold" }))
}

let mut registry = ToolRegistry::default();
if cfg!(feature = "gold-tier") {          // 条件分支动态注册——AgentBuilder 的强项
    registry.register(read_tier);
}
let agent = AgentBuilder::new("member_agent")
    .instructions("你根据用户等级回答特权问题。")
    .with_tool_registry(registry)
    .build()?;
```

**（三）混用**：先用声明式加载「静态骨架」，再用 `AgentBuilder` 局部覆写或补充运行时工具（Workspace / 动态数据源等）。

## 4. 注意事项 / 常见陷阱

| 维度 | 适用声明式 | 适用 AgentBuilder |
|------|-----------|-------------------|
| 使用者 | 非 Rust 开发者 / 运维 / 文案 | Rust 开发者 |
| 变更频率 | 高频、需离线编辑 | 低频、随代码演进 |
| 校验时机 | 运行时解析（需写校验逻辑） | 编译期（类型系统保证） |
| 与逻辑联动 | 弱（映射层） | 强（可直接引 Rust 逻辑/条件分支） |

**常见陷阱**：

- **把敏感信息放进声明式文件**：配置会进代码仓库，别把 API Key 写进 TOML/YAML；应由环境变量注入。
- **声明式做运行时强校验**：JSON 解析是运行时行为，字段拼错要到运行才报错；而对稳定配置以 Rust 编译期校验更安全。
- **两种模式写同一份 Agent 两遍**：会造成行为漂移。选定一种主模式，另一种只做补充。

## 5. 小结

- 配置驱动的、需多人协作编辑的 → 用 `AgentSchema` v1.0 声明式（JSON/YAML/TOML）。
- 代码驱动、需与 Rust 逻辑/条件分支联动的 → 用 `AgentBuilder`。
- 敏感信息不入配置文件；两种模式可混用，但别对同一 Agent 维护两套定义。

下一节：[多智能体最佳实践](multi-agent-best-practices.md)
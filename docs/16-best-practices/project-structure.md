# 项目组织与 Crates

> **本节目标**：学会围绕 rust-agent-framework 的 workspace 结构组织自己的项目，选对 crate 与 feature，避免「一把梭」引入多余依赖。

## 1. 目标

搭建一个可编译、可测试、可演进的最小集成骨架。读完本节后，你应该能回答三个问题：依赖哪些 crate、crate 的加载顺序如何、哪些 feature 该选哪些不选。

## 2. 核心概念

rust-agent-framework 是一组按职责拆分的 crate 集合（`rust_agent_framework` 及其核心/客户端/宏等 crate）。它们的依赖次序大致是一条自上而下的链：

```
rust_agent_framework      ← 汇总入口：AgentBuilder / WorkflowBuilder 等对外门面
    ▲
    │ depends
    │
core / client / macros    ← 核心 trait（IAgent / ITool）、LLM 客户端、#[tool] 宏
```

- **入口 crate（框架本体）**：绝大多数应用只需依赖它，即可获得 `AgentBuilder`、`ChatClientAgent`、`IAgent`、`WorkflowBuilder` 等。
- **核心 crate**：承载 `ITool`、`ToolResult`、`Message` 等抽象，当你直接实现 trait 或类型时才需要显式引入。
- **宏 crate**：`#[tool]` 属性宏的提供者，开宏功能后便可通过入口 crate 一并导出。

依赖方向永远是「上层依赖下层」，不要反过来在核心 crate 里反向 import 框架门面。

## 3. 可运行示例代码片段

一个最小 `Cargo.toml` 依赖配置：

```toml
[dependencies]
rust-agent-framework = { version = "0.1", features = ["tool-macro", "streaming"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

对应的最小可运行骨架：

```rust
use rust_agent_framework::{AgentBuilder, ToolRegistry, WorkflowBuilder};

#[rust_agent_framework::tool(description = "求两数之和")]
async fn add(num1: f64, num2: f64) -> rust_agent_framework::ToolResult {
    let sum = num1 + num2;
    rust_agent_framework::ToolResult::success(serde_json::json!({ "sum": sum }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut registry = ToolRegistry::default();
    registry.register(add);

    let agent = AgentBuilder::new("math_agent")
        .instructions("你是一个计算助手。")
        .with_tool_registry(registry)
        .build()?;

    // 用 WorkflowBuilder 把 Agent 编排进一个单步工作流
    let workflow = WorkflowBuilder::new()
        .sequential()
        .add_agent("math_agent", agent)
        .build()?;

    println!("骨架就绪：{}", workflow.id());
    Ok(())
}
```

## 4. 注意事项 / 常见陷阱

| 项目 | ✅ 应该 | ⛔ 不应该 |
|------|--------|----------|
| crate 依赖 | 只依赖入口 crate，按需开 feature | 直接引入内部实现 crate，造成版本漂移 |
| feature 选择 | 只开用到的 feature（如 `tool-macro`、`streaming`） | 全量开 feature，拖慢编译 |
| crate 顺序 | 保持核心 → 客户端 → 框架的依赖方向 | 在底层 crate 反向依赖高层门面 |
| 异步运行时 | 在应用入口（main）统一配置 tokio | 在库 crate 内硬编码运行时 |

**常见陷阱**：

- **重复引入同一实现**：既用 `AgentBuilder` 又手动构造 `ChatClientAgent`，导致同一功能两套入口，难以维护。建议统一走框架门面。
- **宏未开对应 feature**：用了 `#[tool]` 却未启用宏 feature，会在编译时报「无法解析外部 crate」。
- **盲目引入全部 crate**：编译时间与二进制体积失控。按「用到哪个开哪个」的原则收敛。

## 5. 小结

- 应用层依赖入口 `rust_agent_framework` crate，按需开启 `tool-macro`、`streaming` 等 feature。
- 遵循「核心 → 客户端 → 框架」的依赖方向，避免底层反向依赖高层。
- 用最小骨架起步：`AgentBuilder` 构建 Agent → `ToolRegistry` 注册工具 → `WorkflowBuilder` 完成编排。

下一节：[Agent 设计指南](agent-design.md)
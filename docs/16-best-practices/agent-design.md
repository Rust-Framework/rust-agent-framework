# Agent 设计指南

> **本节目标**：学会设计边界清晰、可测试、可演进的那些——包括单 Agent 与多 Agent 的取舍、记忆组织方式，以及工具的职责划分。

## 1. 目标

读完本节，你应能：判断一个需求该用单 Agent 还是多 Agent；知道如何给它配置历史记忆与技能；能把一张「模糊的大需求」拆成一组接口清晰的小工具。

## 2. 核心概念

**Agent 是「上下文 + 工具」的封装。** 与其问「要不要第二个 Agent」，不如先问「两个 Agent 是否共享不同上下文与工具」。一个 `IAgent` 的核心可配置项包括：

- 指令（instructions）：定义角色与行为边界；
- 上下文提供器（`IContextProvider`）：注入历史、技能等额外上下文；
- 工具集：Agent 能调用的能力，
- 以及由工具触发的审批（`ApprovalRequiredTool`）。

**单 Agent** 适合：任务内聚、工具集一致、上下文单一。**多 Agent** 适合：任务可分域、不同子任务需要不同工具/提示/审批策略的场景。

**记忆（Memory）** 由会话（`ISession` / `AgentSession`）与历史提供器（`InMemoryHistoryProvider`）承载。会话负责在多轮交互间保留 `Message`，历史提供器负责决定哪些历史进入 Prompt。

## 3. 可运行示例代码片段

**（一）设计一个职责内聚的单 Agent：**

```rust
use rust_agent_framework::{AgentBuilder, ToolRegistry};
use rust_agent_framework::providers::{AgentSkillsProvider, InMemoryHistoryProvider};

#[rust_agent_framework::tool(description = "查询天气")]
async fn get_weather(city: String) -> rust_agent_framework::ToolResult {
    rust_agent_framework::ToolResult::success(serde_json::json!({ "city": city }))
}

let mut registry = ToolRegistry::default();
registry.register(get_weather);

let mut provider = AgentBuilder::new("weather_agent")
    .instructions("你只负责回答天气相关问题，其他问题一律拒绝。")
    .with_tool_registry(registry)
    .with_context_provider(InMemoryHistoryProvider::default())
    .with_context_provider(AgentSkillsProvider::from(vec!["技能A", "技能B"]))
    .build()?;
```

**（二）用会话贯穿多轮交互：**

```rust
let mut session = rust_agent_framework::sessions::AgentSession::new("weather_agent");
// 把会话接入 Agent，此后 run 的往返消息由会话保留
provider.appear_live(session); // 示意：将会话绑定到 Agent 状态
let response = provider.run(vec![Message::user("上海今天热吗？")], None, None).await?;
```

工具的职责划分：一个工具做一件确定性的事，返回结构化 JSON（`ToolResult::success(json!(...))`）。不要在工具里塞「会调用另一个 Agent」的逻辑；跨 Agent 协作请交给多智能体编排（见 [多智能体最佳实践](multi-agent-best-practices.md)）。

## 4. 注意事项 / 常见陷阱

| 项目 | ✅ 应该 | ⛔ 不应该 |
|------|--------|----------|
| 单/多 Agent | 先按「上下文+工具是否一致」判断，再拆分 | 一上来就为每个功能建一个 Agent |
| 记忆 | 用会话（ISession）承载多轮状态 | 把状态塞进全局变量 / 静态量 |
| 指令 | 只给与职责相关的行为约束 | 把所有 Agent 的指令堆在一个 Prompt 里 |
| 工具 | 一个工具一件确定性的事，返回结构化 JSON | 工具内隐式调用别的 Agent / 阻塞长时间运行 |
| 审批 | 对危险工具用 `ApprovalRequiredTool` 包装 | 所有工具都要求审批，拖垮体验 |

**常见陷阱**：

- **「全都要」Agent**：一个 Agent 塞满所有工具与提示，Prompt 膨胀、工具选择变差。宁可拆成两个职责单一的 Agent 再编排。
- **状态放错地方**：把会话历史放进 `static`，进程重启即丢。应交给 `AgentSession` / 会话存储管理。
- **工具返回自然语言而非结构化数据**：LLM 解析不稳定。`ToolResult` 应返回可解析的 JSON。

## 5. 小结

- 按「上下文 + 工具是否一致」决定单 / 多 Agent，不要为了拆分而拆分。
- 用会话 + 历史提供器管理记忆；用技能提供器注入能力描述。
- 工具保持小而确定，危险操作用 `ApprovalRequiredTool` 包裹。

下一节：[常见陷阱与排查](common-pitfalls.md)
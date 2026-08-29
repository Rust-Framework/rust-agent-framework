# 性能调优

> **本节目标**：找对性能优化着力点——流式处理、会话/历史缓存、Provider 复用——不重复造轮子。

## 1. 目标

读完本节，你应能：说出框架「已经替你优化好」的地方，避免在错误处动手；并对流式消费、历史窗口、Provider 复用几个关键杠杆做出正确取舍。

## 2. 核心概念

- **流式（streaming）**：边生成边返回增量。真正该优化的不是「加快生成」，而是「尽快消费增量、尽早释放处理」。等待整条流才返回，等于放弃流式全部收益。
- **缓存（caching）**：会话历史、技能描述、上下文提供器的产物都可以复用。重复为同一上下文做昂贵计算是常见的性能浪费。
- **Provider 复用（provider reuse）**：`IContextProvider`、`InMemoryHistoryProvider` 等若被反复创建，会反复重建内部状态。共享、复用单例实例更省。

## 3. 可运行示例代码片段

**（一）流式：边收边画，尽早消费**

```rust
use rust_agent_framework::{AgentBuilder, Message};

let agent = AgentBuilder::new("stream").instructions("你是一个助手。").build()?;
let stream = agent.run(vec![Message::user("写一段话")], None, None).await?;

// 逐帧即时消费，而不是等全文；这样下游（UI/日志）感知延迟最低
while let Some(update) = stream.next().await {
    if let Some(delta) = update.text_delta() {
        // 追加到缓冲区 / 推送给前端
    }
}
```

**（二）复用历史提供器实例**

```rust
use rust_agent_framework::providers::InMemoryHistoryProvider;
use std::sync::Arc;

// 一次创建，多处复用，避免每次 run 重建
let history = Arc::new(InMemoryHistoryProvider::default());
// 把同一个 Arc 传给多个 Agent（若上下文策略允许）
```

**（三）控制上下文规模：不把整段历史无差别塞进 Prompt**

```rust
use rust_agent_framework::providers::InMemoryHistoryProvider;
// 通过配置窗口大小限制进入 Prompt 的历史条数，降低 token 成本与首 token 延迟
let history = InMemoryHistoryProvider::default().with_window(20);
```

## 4. 注意事项 / 常见陷阱

| 着力点 | ✅ 应该 | ⛔ 别把力气用在这 |
|--------|--------|------------------|
| 流式 | 逐帧消费、尽早提交/渲染 | 等整条流 `.await` 完再处理（等于没用流式） |
| 缓存 | 复用会话历史、复用 Provider 实例 | 每次 run 重建所有上下文 |
| token | 用历史窗口截断、精简技能描述 | 手工拼接巨型 Prompt |
| 并发 | 合理的工具并行取决于框架编排（如 `ConcurrentWorkflow`） | 在工具内部硬编码 `spawn` 去抢全局并发 |

**常见陷阱**：

- **把「加快 LLM 生成」误认为是优化点**：生成速度由模型决定，你能优化的是「消费与组装」的节奏。
- **过度复用一个共享可变 Provider**：多 Agent 并发共享可变上下文可能串号。共享要基于只读/克隆语义，可变状态仍按会话隔离。
- **无脑缓存全局数据**：静态共享但内容会变（如过期 skills），缓存了反而返错。给缓存设失效或按会话隔离。

## 5. 小结

- 流式优化 = 逐帧即时消费，别等到全文。
- 缓存优化 = 复用历史与 Provider 实例，但要按会话隔离可变状态。
- 不要试图优化框架已内置的环节（如工具调用循环的并发），先定位真正瓶颈。

下一节：[声明式 vs AgentBuilder](declarative-vs-builder.md)
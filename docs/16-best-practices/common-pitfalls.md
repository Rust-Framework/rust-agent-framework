# 常见陷阱与排查

> **本节目标**：快速识别并修复流式输出、工具注册、会话生命周期这三类高频问题。

## 1. 目标

读完本节，你应能：遇到「流式没增量」「工具调不到」「会话串号/丢失」时，数分钟内定位根因并修复。

## 2. 核心概念

- **流式**：`IAgent::run` 返回一个增量事件流。若你在消费流之前就 `.await` 了整个流，或中途 `break`，都会看不到完整增量。
- **工具注册**：工具必须被 `ToolRegistry` 注册，**并在 Agent 构建时注入**，LLM 才知道有这一步可用。
- **会话生命周期**：`ISession` / `AgentSession` 负责多轮状态。会话创建、绑定、销毁的时机错了，就会出现「上一轮的回答影响下一轮」或「状态丢失」。

## 3. 可运行示例代码片段

**（一）正确地消费流式输出：**

```rust
use rust_agent_framework::{AgentBuilder, Message};

let agent = AgentBuilder::new("demo").instructions("你是一个助手。").build()?;

let mut stream = agent.run(vec![Message::user("你好")], None, None).await?;
// 逐帧消费增量，不要直接 .await 整个流
while let Some(update) = stream.next().await {
    if let Some(delta) = update.text_delta() {
        print!("{delta}");
    }
}
println!();
```

**（二）注册工具后再交给 Agent：**

```rust
use rust_agent_framework::{AgentBuilder, ToolRegistry};

#[rust_agent_framework::tool(description = "获取股票价格")]
async fn get_stock_price(symbol: String) -> rust_agent_framework::ToolResult {
    rust_agent_framework::ToolResult::success(serde_json::json!({ "symbol": symbol }))
}

let mut registry = ToolRegistry::default();
registry.register(get_stock_price);   // ① 注册到注册表

let agent2 = AgentBuilder::new("trader")
    .instructions("你使用股票工具作答。")
    .with_tool_registry(registry)     // ② 构建时注入——忘掉这步工具等于没注册
    .build()?;
```

**（三）管理会话生命周期（创建 → 复用 → 结束）：**

```rust
use rust_agent_framework::sessions::AgentSession;

// 每个用户/每轮对话维护一个会话，而不是每次都新建
let session = AgentSession::new("customer_support");
// 将历史写入会话后，多轮交互才能上下文连贯
```

## 4. 注意事项 / 常见陷阱

| 现象 | 常见根因 | 排查方向 |
|------|----------|----------|
| 只打印最终结果、看不到增量 | 消费流前 `.await` 整个流 / 提前 `break` | 用 `while let Some` 逐帧消费，检查是否完整 await |
| Agent 无论如何都不调某个工具 | 工具没注册 / 构建时没注入 / 描述不清 | 检查 `ToolRegistry.register` 与 `with_tool_registry` |
| 工具抛错导致流中断 | 使用 `Err(AgentError)` 而非业务失败 | 业务失败用 `ToolResult::error`，框架错误才 `Err` |
| 上一轮回答污染下一轮 | 会话宽度下限内历史未截断 | 给会话/历史提供器配置合理窗口 |
| 手动重启后状态消失 | 用内存会话存储 | 改用文件/隔离会话存储持久化 |

**Do / Don't**：

- ✅ 流式：逐帧消费增量，及时处理 `text_delta`。
- ⛔ 不要在工具 `execute` 里长时间阻塞而不返回，会卡住工具调用循环。
- ✅ 为每个逻辑会话复用同一个 `AgentSession`。
- ⛔ 别把会话对象放在 `static` 里做「零拷贝复用」，注意并发与生命周期。

## 5. 小结

- 流式要「逐帧消费」，工具要「注册 + 注入」两步都做齐，会话要「建一次、复用全程」。
- 业务失败用 `ToolResult::error`，框架异常才用 `Err(AgentError)`。
- 排查顺序：先看工具是否注册 → 再看流是否完整消费 → 最后看会话生命周期。

下一节：[性能调优](performance-tuning.md)
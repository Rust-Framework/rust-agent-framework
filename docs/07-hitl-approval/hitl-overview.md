# 7.1 人机协同（HITL）概述

## 什么是 HITL

**Human-In-The-Loop（HITL）** 是一种将人工判断嵌入自动化流程的设计模式。在 AI Agent 的语境下，HITL 意味着 Agent 在执行某些操作之前，必须暂停并等待人工操作员的审批。只有获得批准后，操作才会实际执行。

### 为什么 Agent 需要 HITL

1. **安全性**：Agent 可能被赋予文件系统访问、命令执行等能力，错误的工具调用可能造成不可逆的损害（删除文件、执行危险命令）。
2. **合规性**：金融、医疗等行业要求关键操作必须有审计记录和人工授权。
3. **信任建立**：让用户对 Agent 行为有可见性和控制权，逐步建立信任。
4. **渐进式自动化**：先在低风险场景全自动，高风险场景人工审批，随着系统成熟逐步减少人工介入。

### RAF 中的 HITL 场景示例

- Agent 尝试在工作区范围外读写文件
- Agent 尝试执行系统命令（`run_command`）
- Agent 在 `ApproveOutside` 策略下访问跨范围路径
- 开发者显式用 `ApprovalRequiredTool` 包装任意工具

## RAF 的 HITL 设计

### 核心三要素

RAF 的 HITL 机制由三个概念紧密配合构成：

1. **`ApprovalRequiredTool`** — 工具包装器。将任意 `Arc<dyn ITool>` 包装后，其 `requires_approval()` 返回 `true`。这个标记在运行时由 `FunctionInvokingChatClient` 检测。

2. **`ToolApprovalRequest` / `ToolApprovalResponse`** — 审批通信协议。当检测到需要审批的工具调用时，`FunctionInvokingChatClient` 发出的 `AgentResponseUpdate::ToolApprovalRequest` 事件包含 `call_id`、`name`、`arguments`、`description`。调用者以 `ToolApprovalResponse`（包含 `call_id`、`approved`、`reason`）回传审批决定。

3. **`FinishReason::AwaitingApproval`** — 流暂停信号。这是一个特殊的结束原因，表示 Agent 执行流已暂停，等待人工审批。会话保留完整的消息上下文（包括 `assistant(tool_calls)` 消息），使得恢复执行时无需重新传递消息。

### 设计理念：为什么在装饰器层实现

RAF 将 HITL 审批逻辑放在 `FunctionInvokingChatClient`（一个 `IChatClient` 装饰器）而非 Agent 层，这是经过深思熟虑的架构决策：

```
┌─────────────────────────────────────────────────┐
│               ChatClientAgent (Agent 层)          │
│  职责：指令组装、Provider 调用、压缩、会话管理      │
└────────────────────┬────────────────────────────┘
                     │
         ┌───────────▼───────────┐
         │ FunctionInvoking      │  ← 装饰器层
         │   ChatClient          │    审批 + 工具循环
         │  · 工具调用检测        │    在此实现
         │  · 审批请求发出        │
         │  · 审批响应处理        │
         │  · 工具执行            │
         └───────────┬───────────┘
                     │
         ┌───────────▼───────────┐
         │ DeepSeekChatClient /  │  ← 叶子客户端
         │ OpenAiChatClient      │    HTTP/SSE 传输
         └───────────────────────┘
```

**选择装饰器层的原因**：

1. **关注点分离**：Agent 层负责"调度"（组装消息、管理会话），装饰器层负责"执行"（工具调用循环、审批）。Agent 不需要知道工具是否需要审批——它只是把消息传给 `IChatClient`。
2. **可组合性**：通过 `ChatClientBuilder` 管道，审批逻辑可以与任何 LLM 提供商结合，不需要修改 Agent 代码。
3. **消息上下文自然保持**：工具调用循环在装饰器内部累积 `assistant(tool_calls)` → `tool(result)` 消息对，审批暂停时这些消息自然地保存在循环状态中，恢复时无需外部重建。

### 工具包装：ApprovalRequiredTool

`ApprovalRequiredTool` 是一个简单的包装器结构体：

```rust
// crates/core/src/tool.rs

pub struct ApprovalRequiredTool {
    pub inner: Arc<dyn ITool>,
}

impl ITool for ApprovalRequiredTool {
    fn name(&self) -> &str { self.inner.name() }
    fn description(&self) -> &str { self.inner.description() }
    fn parameters(&self) -> serde_json::Value { self.inner.parameters() }
    // ... execute 委托给 inner ...

    fn requires_approval(&self) -> bool {
        true  // ← 关键：覆盖默认的 false
    }
}
```

它实现了完整的 `ITool` trait，将所有方法委托给内部工具，唯独覆盖 `requires_approval()` 返回 `true`。这意味着 `FunctionInvokingChatClient` 在执行前检查此方法时，会触发审批流程。

**使用方式**：

```rust
// Agent A：自动执行（开发环境）
builder.with_tool(RunCommand);

// Agent B：需要审批（生产环境）
builder.with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand)));
```

### FinishReason::AwaitingApproval

`FinishReason` 枚举中定义了一个专门的变体：

```rust
pub enum FinishReason {
    Stop,           // 正常结束
    Length,         // 达到长度限制
    ToolCalls,      // 包含工具调用（内部使用，对消费者过滤）
    ContentFilter,  // 被内容过滤器拦截
    AwaitingApproval, // ← 审批暂停
    MaxRounds,      // 达到最大轮次
    Other(String),  // 其他
}
```

当调用者收到 `FinishReason::AwaitingApproval` 时，应该：
1. 从 `ToolApprovalRequest` 事件中获取审批信息
2. 向用户展示审批请求
3. 收集用户的审批决定
4. 构造 `AgentRunOptions` 并携带 `tool_approval_responses` 重新调用 `agent.run()`

### 与 MAF 的对应关系

RAF 的 HITL 设计直接对齐 Microsoft Agent Framework (MAF)：

| RAF | MAF (.NET) |
|-----|-----------|
| `ApprovalRequiredTool` | `ApprovalRequiredAIFunction` |
| `AgentResponseUpdate::ToolApprovalRequest` | `FunctionApprovalRequestContent` |
| `ToolApprovalResponse` | `FunctionApprovalResponseContent` |
| `FinishReason::AwaitingApproval` | `FinishReason.AwaitingApproval` |

这种对齐确保了熟悉 MAF 的开发者可以快速理解 RAF 的 HITL 机制，同时保持了 Rust 生态的习惯用法（如 `Arc<dyn ITool>` 替代 C# 的接口引用，`mpsc::channel` 替代事件委托）。

## 本章后续

- [7.2 审批流完整链路](approval-flow.md)：用 mermaid 时序图完整展示审批流程
- [7.3 ToolApprovalRequest/Response API](tool-approval-api.md)：审批协议的字段详解与代码示例
- [7.4 中断恢复与取消机制](resume-cancel.md)：审批暂停后如何恢复执行与主动取消

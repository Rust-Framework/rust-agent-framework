# 7.3 ToolApprovalRequest/Response API

## 概述

审批通信协议由两个核心结构体定义：**请求**（`AgentResponseUpdate::ToolApprovalRequest`）和**响应**（`ToolApprovalResponse`）。它们分别由框架发出和由调用者构造。

## ToolApprovalRequest — 审批请求

审批请求以 `AgentResponseUpdate::ToolApprovalRequest` 变体在流中发出。它不是一个独立的顶层结构体，而是 `AgentResponseUpdate` 枚举的一个变体：

```rust
// crates/core/src/message.rs

pub enum AgentResponseUpdate {
    // ... 其他变体 ...

    /// 工具调用需要人工审批才能执行
    ToolApprovalRequest {
        /// 唯一标识此次工具调用，与 ToolCall.id 对应
        call_id: String,
        /// 工具名称，如 "run_command", "write_file"
        name: String,
        /// 工具调用参数（已解析为 serde_json::Value）
        arguments: serde_json::Value,
        /// 工具描述文本，从 ITool::description() 获取
        description: String,
    },

    // ...
}
```

### 字段详解

| 字段 | 类型 | 说明 |
|------|------|------|
| `call_id` | `String` | 全局唯一的工具调用 ID。与 `ToolCallStartContent.call_id` 和 `ToolCall.id` 对应。用于匹配 `ToolApprovalResponse`。 |
| `name` | `String` | 工具名称。在工具注册表中查找对应的 `ITool` 实现。 |
| `arguments` | `serde_json::Value` | 已解析的参数。可能为 `String`（原始 JSON 字符串）或 `Object`（已解析的结构体）。推荐用 `arguments.as_str()` 获取字符串形式用于展示。 |
| `description` | `String` | 工具的人类可读描述，从 `ITool::description()` 获取。用于 UI 展示，帮助用户做出审批决策。 |

### 审批请求的产生位置

审批请求在 `FunctionInvokingChatClient` 的 spawned task 中产生，发生在工具调用累积完毕后、实际执行之前：

```rust
// crates/framework/src/decorators/invoke.rs

// 1. 检查是否有任何工具需要审批
let any_requires_approval = tool_calls.iter().any(|tc| {
    tools_for_execution
        .iter()
        .any(|t| t.name() == tc.name && t.requires_approval())
});

// 2. 如果有，为每个工具调用发出 ToolApprovalRequest
if any_requires_approval {
    for tc in &tool_calls {
        let tool = tools_for_execution.iter().find(|t| t.name() == tc.name);
        let desc = tool.map(|t| t.description().to_string()).unwrap_or_default();
        let args: serde_json::Value =
            serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null);

        tx.send(Ok(AgentResponseUpdate::ToolApprovalRequest {
            call_id: tc.id.clone(),
            name: tc.name.clone(),
            arguments: args,
            description: desc,
        })).await;
    }

    // 3. 跟上 Finish(AwaitingApproval) 结束流
    tx.send(Ok(AgentResponseUpdate::Finish {
        finish_reason: FinishReason::AwaitingApproval,
        usage: None,
    })).await;
}
```

### 在 AgentResponseConverter 中的处理

当 `ToolApprovalRequest` 到达 `AgentResponseConverter` 时，它被转换为可读的文本提示：

```rust
// crates/framework/src/converter.rs

AgentResponseUpdate::ToolApprovalRequest { name, arguments, .. } => {
    let args_display = arguments
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| serde_json::to_string(&arguments).unwrap_or_default());
    contents.push(Content::Text(TextContent {
        meta: self.build_meta(),
        delta: format!("\n[Approval required: {}({})]\n", name, args_display),
    }));
}
```

## ToolApprovalResponse — 审批响应

审批响应由调用方构造，通过 `AgentRunOptions.tool_approval_responses` 传递给下一次 `agent.run()` 调用：

```rust
// crates/core/src/tool.rs

/// 调用者对 ToolApprovalRequest 的响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolApprovalResponse {
    /// 匹配对应 ToolApprovalRequest 中的 call_id
    pub call_id: String,
    /// true = 批准执行，false = 拒绝
    pub approved: bool,
    /// 拒绝原因（可选，会反馈给 LLM）
    pub reason: Option<String>,
}
```

### 字段详解

| 字段 | 类型 | 说明 |
|------|------|------|
| `call_id` | `String` | 必须匹配 `ToolApprovalRequest.call_id`。用于将响应关联到具体的工具调用。 |
| `approved` | `bool` | `true`：批准执行 → 工具正常执行，结果注入 LLM 对话。`false`：拒绝 → 工具不执行，拒绝原因作为错误反馈给 LLM。 |
| `reason` | `Option<String>` | 拒绝原因。仅在 `approved: false` 时有意义。原因会被格式化为 `"Rejected: {reason}"` 并作为 tool 角色的消息注入对话，让 LLM 了解用户为什么拒绝了该操作。 |

### 传递机制

审批响应通过 `AgentRunOptions` → `ChatClientRunOptions` → `FunctionInvokingChatClient` 的传播链路到达执行层：

```
AgentRunOptions.tool_approval_responses
    ↓ AgentRunOptions::to_chat_client_run_options()
ChatClientRunOptions.tool_approval_responses
    ↓ ChatClientAgent.run() → client_opts
FunctionInvokingChatClient.run(messages, options)
    ↓ 检查 options.tool_approval_responses
审批恢复逻辑
```

```rust
// crates/core/src/run_options.rs

impl AgentRunOptions {
    pub fn to_chat_client_run_options(&self) -> ChatClientRunOptions {
        ChatClientRunOptions {
            // ... 其他字段 ...
            tool_approval_responses: self.tool_approval_responses.clone(),
            cancelled: self.cancelled.clone(),
        }
    }
}
```

## 代码示例

### 示例 1：基本审批流程

```rust
use rust_agent_core::{
    AgentRunOptions, ToolApprovalResponse,
    ApprovalRequiredTool, FinishReason,
};
use rust_agent_client::{DeepSeekChatClient, ChatClientOptions};
use rust_agent_framework::{AgentBuilder, FunctionInvokingChatClient};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // 构建会话
    let session = /* 创建 ISession */;

    // 构建带审批的 Agent
    let agent = AgentBuilder::new("my-agent")
        .with_chat_client(/* chat_client */)
        .with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand::default())))
        .with_session(session.clone())
        .build()
        .unwrap();

    // 第一轮：触发审批
    let mut stream = agent.run(
        vec![ChatMessage::user("帮我删除 /tmp/logs 目录")],
        Some(session.clone()),
        None,
    ).await.unwrap();

    let mut approval_requests = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk.unwrap() {
            AgentResponseResult { finish_reason: Some(FinishReason::AwaitingApproval), .. } => {
                break; // 审批暂停
            }
            result => {
                // 收集 ToolApprovalRequest（通过 Content::Text 的文本提示）
                for content in &result.contents {
                    if let Content::Text(t) = content {
                        if t.delta.contains("[Approval required:") {
                            // 解析审批请求信息
                            println!("审批请求: {}", t.delta);
                        }
                    }
                }
            }
        }
    }

    // 构造审批响应
    let response = ToolApprovalResponse {
        call_id: "call_1".to_string(), // 从审批请求中获取
        approved: true,
        reason: None,
    };

    // 第二轮：恢复执行
    let options = AgentRunOptions::new()
        .with_tool_approval_responses(vec![response]);

    let mut stream = agent.run(
        vec![ChatMessage::user("继续")],
        Some(session.clone()),
        Some(options),
    ).await.unwrap();

    // 消费恢复后的流
    while let Some(chunk) = stream.next().await {
        // 处理最终结果
    }
}
```

### 示例 2：拒绝工具调用

```rust
// 拒绝并给出原因
let response = ToolApprovalResponse {
    call_id: "call_2".to_string(),
    approved: false,
    reason: Some("该命令会删除生产数据库，请使用更安全的命令".to_string()),
};

let options = AgentRunOptions::new()
    .with_tool_approval_responses(vec![response]);
```

拒绝后，`FunctionInvokingChatClient` 会生成如下消息注入对话：

```json
{
    "role": "tool",
    "tool_call_id": "call_2",
    "content": "Rejected: 该命令会删除生产数据库，请使用更安全的命令"
}
```

LLM 收到这条消息后可以理解用户拒绝了该操作以及原因，从而给出替代方案。

### 示例 3：多工具审批

```rust
// 两个工具调用都需要审批
let responses = vec![
    ToolApprovalResponse {
        call_id: "call_echo".to_string(),
        approved: true,
        reason: None,
    },
    ToolApprovalResponse {
        call_id: "call_rm".to_string(),
        approved: false,
        reason: Some("删除文件前请先备份".to_string()),
    },
];

let options = AgentRunOptions::new()
    .with_tool_approval_responses(responses);
```

## 数据流总结

```mermaid
graph LR
    A[LLM 输出<br/>tool_calls] --> B{FICC 检查<br/>requires_approval?}
    B -->|false| C[直接执行工具]
    B -->|true| D[发出 ToolApprovalRequest<br/>+ AwaitingApproval]
    D --> E[调用方收集<br/>审批决定]
    E --> F[构造 ToolApprovalResponse]
    F --> G[AgentRunOptions<br/>.tool_approval_responses]
    G --> H[agent.run() 恢复]
    H --> I{FICC 处理}
    I -->|approved: true| J[执行工具<br/>追加 tool(result)]
    I -->|approved: false| K[追加 tool(rejected) 消息]
    J --> L[继续 LLM 循环]
    K --> L
```

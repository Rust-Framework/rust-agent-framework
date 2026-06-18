# 8.2 WorkspaceContextProvider

## 概述

`WorkspaceContextProvider` 是工作区管理的主入口。它实现了 `IContextProvider` trait，在 Agent 的 `on_invoking()` 阶段向 LLM 注入工作区指令，并将工具配置为工作区感知。它与 `AgentSkillsProvider` 设计模式一致，但由于工作区 scope 是 Agent 运行的必要条件，scope 通过**构造函数注入**（编译期保证必须提供）。

```rust
// crates/framework/src/context_providers/workspace.rs

pub struct WorkspaceContextProvider {
    scope: Arc<WorkspaceScope>,
    tools: Vec<Arc<dyn ITool>>,
}
```

## 构造函数注入

### 基本用法

```rust
let scope = WorkspaceScope::new("/project", "my-project")
    .with_policy(ScopePolicy::ApproveOutside);

let provider = WorkspaceContextProvider::new(Arc::new(scope))
    .add_tool(ReadFile { scope: None })
    .add_tool(WriteFile { scope: None })
    .add_tool(RunCommand { scope: None, timeout_secs: None });
```

### 与 AgentBuilder 集成

```rust
use rust_agent_framework::{
    AgentBuilder, WorkspaceContextProvider,
};
use rust_agent_core::{WorkspaceScope, ScopePolicy};

let scope = WorkspaceScope::new("/workspace", "code-review")
    .with_policy(ScopePolicy::ApproveOutside);

let agent = AgentBuilder::new("workspace-agent")
    .with_instructions("你是一个代码审查助手")
    .with_chat_client(/* ... */)
    .with_context_provider(WorkspaceContextProvider::new(Arc::new(scope))
        .add_tool(ReadFile { scope: None })
        .add_tool(WriteFile { scope: None })
        .add_tool(ListFiles { scope: None })
        .add_tool(InspectFile { scope: None })
        .add_tool(FindFiles { scope: None })
        .add_tool(SearchFile { scope: None })
        .add_tool(RunCommand {
            scope: None,
            timeout_secs: Some(30),
        })
    )
    .build()?;
```

## add_tool() 方法详解

`add_tool()` 方法内部执行两步处理：

```rust
pub fn add_tool(mut self, tool: impl ITool + 'static) -> Self {
    let mut tool: Arc<dyn ITool> = Arc::new(tool);

    // Step 1: scope 注入（检测 IScopeTool）
    if let Some(scoped) = try_inject_scope(&tool, Arc::clone(&self.scope)) {
        tool = scoped;
    }

    // Step 2: 审批包裹（按策略）
    if self.scope.policy == ScopePolicy::ApproveOutside {
        tool = Arc::new(ApprovalRequiredTool::new(tool));
    }

    self.tools.push(tool);
    self
}
```

```mermaid
graph TD
    A[add_tool(ITool)] --> B{工具实现<br/>IScopeTool?}
    B -->|是| C[调用 create_scoped(scope)<br/>注入 WorkspaceScope]
    B -->|否| D[保持原工具]
    C --> E{ScopePolicy?}
    D --> E
    E -->|ApproveOutside| F[包裹 ApprovalRequiredTool]
    E -->|AllowAll / DenyOutside| G[直接使用]
    F --> H[加入 tools 列表]
    G --> H
```

### Step 1: IScopeTool 自动检测

`try_inject_scope()` 通过 `AsAny` 下转型检测工具是否实现了 `IScopeTool`：

```rust
fn try_inject_scope(
    tool: &Arc<dyn ITool>,
    scope: Arc<WorkspaceScope>,
) -> Option<Arc<dyn ITool>> {
    use rust_agent_core::AsAny;
    let any = tool.as_any();

    // 为每个已知工具类型尝试下转型并调用 create_scoped
    if any.downcast_ref::<ReadFile>().is_some() {
        let dummy = ReadFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    if any.downcast_ref::<WriteFile>().is_some() {
        let dummy = WriteFile { scope: None };
        return Some(dummy.create_scoped(scope));
    }
    // ... 其他工具类型类似 ...
    None
}
```

### Step 2: 策略驱动的审批包装

当策略为 `ApproveOutside` 时，工具被包装为 `ApprovalRequiredTool`。结合 [第 7 章](../07-hitl-approval/) 的 HITL 机制：

- Agent 尝试跨范围操作时，`path_guard` 返回 `ScopeStatus::OutsideScope`
- `FunctionInvokingChatClient` 检测到 `requires_approval() == true`
- 发出 `ToolApprovalRequest` 事件并暂停流
- 用户审批后恢复执行

## build_instructions()

`build_instructions()` 生成注入 system prompt 的工作区指令文本：

```rust
fn build_instructions(&self) -> String {
    let policy_desc = match self.scope.policy {
        ScopePolicy::AllowAll => "无限制（所有路径均可访问）",
        ScopePolicy::ApproveOutside => "跨范围审批（工作区外的操作需用户审批后方可执行）",
        ScopePolicy::DenyOutside => "禁止越界（工作区外操作直接拒绝）",
    };
    format!(
        "## 工作区\n\
         名称: {name}\n\
         根路径: {root}\n\
         越界策略: {policy}\n\n\
         - 相对路径在工作区内解析\n\
         - 绝对路径若在工作区外，工具返回中 scope 字段会标明 outside_workspace\n\
         - 每个工具返回均包含 scope 字段以标明操作范围",
        name = self.scope.name,
        root = self.scope.root.display(),
        policy = policy_desc,
    )
}
```

## on_invoking() 实现

`WorkspaceContextProvider` 在 `on_invoking()` 中同时提供指令注入和工具注入：

```rust
#[async_trait]
impl IContextProvider for WorkspaceContextProvider {
    fn name(&self) -> &str {
        "WorkspaceContextProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextInjection> {
        // 会话级持久化（仅首次，用于审计/调试）
        let state = ProviderState::<WorkspaceState>::new("WorkspaceContextProvider");
        let ws = state.get_or_init(session);
        if ws.scope_name.is_empty() {
            let _ = state.save(session, &WorkspaceState {
                scope_name: self.scope.name.clone(),
                scope_root: self.scope.root.to_string_lossy().to_string(),
                policy: format!("{:?}", self.scope.policy),
            });
        }

        Ok(ContextInjection {
            instructions: Some(self.build_instructions()),
            tools: self.tools.clone(),
            ..Default::default()
        })
    }
}
```

`ContextInjection` 返回的数据结构：

```rust
pub struct ContextInjection {
    /// 注入 system prompt 的指令文本
    pub instructions: Option<String>,
    /// 注入执行层的工具列表
    pub tools: Vec<Arc<dyn ITool>>,
    /// 注入历史的消息（如技能文档）
    pub messages: Vec<ChatMessage>,
    /// 是否替换而非追加前面的消息
    pub replace_messages: bool,
}
```

## 完整使用示例

以下示例展示了一个带有工作区管理的 Agent 的完整构建过程：

```rust
use std::sync::Arc;
use rust_agent_core::{
    WorkspaceScope, ScopePolicy, AgentRunOptions,
    ChatClientBuilder, ChatMessage,
};
use rust_agent_client::{
    DeepSeekChatClient, ChatClientOptions,
};
use rust_agent_framework::{
    AgentBuilder, FunctionInvokingChatClient,
    WorkspaceContextProvider,
};

// 1. 定义工作区范围
let scope = WorkspaceScope::new("/home/user/myapp", "myapp")
    .with_policy(ScopePolicy::ApproveOutside);

// 2. 创建 LLM 客户端
let llm_options = ChatClientOptions::deepseek(
    "deepseek-chat",
    std::env::var("DEEPSEEK_API_KEY").unwrap(),
);
let llm_client = DeepSeekChatClient::new(llm_options).unwrap();

// 3. 用 ChatClientBuilder 构建管道
let pipeline = ChatClientBuilder::new()
    .leaf(Arc::new(llm_client))
    .build()
    .unwrap();

// 4. 构建 Agent（工作区通过 ContextProvider 注入）
let agent = AgentBuilder::new("workspace-agent")
    .with_instructions("你是文件管理助手")
    .with_chat_client(pipeline)
    // 工作区 Provider 自动处理 IScopeTool 注入和策略包裹
    .with_context_provider(WorkspaceContextProvider::new(Arc::new(scope))
        .add_tool(ReadFile { scope: None })
        .add_tool(WriteFile { scope: None })
        .add_tool(RunCommand {
            scope: None,
            timeout_secs: Some(30),
        })
    )
    .build()
    .unwrap();

// 5. 运行 Agent
let session = agent.create_session();
let messages = vec![ChatMessage::user("读取 /etc/hosts 的内容")];

let mut stream = agent.run(
    messages,
    Some(session.clone()),
    None,
).await.unwrap();

// 6. 消费流 — 跨范围操作会触发审批
while let Some(chunk) = stream.next().await {
    // 处理结果...
}
```

## 与 AgentSkillsProvider 的对比

| 特性 | WorkspaceContextProvider | AgentSkillsProvider |
|------|------------------------|---------------------|
| 注入内容 | 工作区指令 + 工作区感知工具 | 技能指令 + 技能工具 |
| 工具处理 | 自动 scope 注入 + 策略包裹 | 直接添加 |
| 构造函数 | `new(Arc<WorkspaceScope>)` | `new()` |
| 持久化状态 | `WorkspaceState`（scope 信息） | 技能数据 |
| 策略驱动 | 是（ScopePolicy） | 否 |

## 归纳

`WorkspaceContextProvider` 通过以下机制实现了声明式的工作区管理：

1. **构造函数注入 scope**：编译期保证必须提供，避免运行时遗漏。
2. **自动 IScopeTool 检测**：无需手动调用 `create_scoped()`，Provider 自动处理。
3. **策略驱动审批**：`ApproveOutside` 策略下工具自动包装为 `ApprovalRequiredTool`，与第 7 章的 HITL 机制无缝集成。
4. **指令注入**：工作区信息通过 system prompt 告知 LLM，帮助其做出合理的路径决策。

# 8.5 声明式工作区配置与工具联动

## 概述

前四节通过 `AgentBuilder` + `WorkspaceContextProvider` 手工构建工作区感知的 Agent。从 v0.1.0 起，RAF 支持通过 YAML / JSON / TOML 声明式配置文件定义工作区边界，将工作区配置、工具选择、审批策略统一在一个文件中管理，无需修改 Rust 代码。

## 全链路：从 YAML 到 HITL 审批

下面是一个完整链路——声明式工作区 + 文件系统工具 + 跨范围审批：

```mermaid
flowchart LR
    A["agent.yaml<br/>(contexts.workspace<br/>+ tools.file)"] --> B[DeclAgentBuilder]
    B --> C[WorkspaceContextProvider<br/>自动创建]
    B --> D[ToolResolver<br/>解析 file 工具]
    C --> E["注入 system prompt<br/>(工作区边界)"]
    D --> F[IScopeTool 实例<br/>(scope: None)]
    E --> G[AgentBuilder]
    F --> G
    G --> H["ChatClientAgent<br/>(路径守卫 + 审批)"]
    H --> I{路径在工作区内?}
    I -->|是| J[直接执行]
    I -->|否 + ApproveOutside| K["ToolApprovalRequest<br/>→ HITL 审批"]
    I -->|否 + DenyOutside| L[ToolResult::error]
```

## 配置文件：agent.yaml

```yaml
kind: prompt
name: workspace-agent
description: 工作区管理的全栈开发助手
model:
  id: agnes-2.0-flash
  provider: openai
  connection:
    kind: key
    api_key: $AGNES_API_KEY
    endpoint: https://apihub.agnes-ai.com/v1
instructions: |
  你是一个工作区感知的开发助手。
  
  重要规则：
  - 所有文件操作在指定工作区内进行
  - 工作区外路径在工具返回中会标注 outside_workspace
  - 若操作被拒绝，重新评估方案

# ── 工作区声明 ──
contexts:
  - kind: workspace
    name: my-project
    config:
      root: /home/dev/myapp
      policy: approve       # 工作区外 → 触发审批

# ── 工具声明（description 由宏内置，无需手写）──
tools:
  - kind: file
    name: read_file
  - kind: file
    name: write_file
  - kind: file
    name: edit_file
  - kind: file
    name: list_files
  - kind: file
    name: search_file
  - kind: file
    name: find_files
  - kind: file
    name: run_command

maxToolRounds: 15
```

## 工具路由规则（关键设计）

当 `contexts` 中存在 `workspace` 声明时，`DeclAgentBuilder` 会**自动分类并路由工具**：

```mermaid
flowchart TD
    A["tools: 全部声明式工具"] --> B{contexts 中<br/>有 workspace?}
    B -->|否| C[所有工具 → AgentBuilder.with_tool()<br/>普通注册]
    B -->|是| D{工具实现<br/>IScopeTool?}
    D -->|是| E["→ WorkspaceContextProvider<br/>  .add_tool_arc() 注册<br/>  (scope注入 + 审批包裹)"]
    D -->|否| F[→ AgentBuilder.with_tool()<br/>普通注册]
    E --> G["运行时 on_invoking():<br/>  ContextResult.tools 注入<br/>  + system prompt 边界指令"]
    F --> G
```

**路由规则**：

| 工具类别 | 是否实现 IScopeTool | 有 workspace 时的注册路径 | 获得的能力 |
|---------|:---:|------|------|
| `read_file`, `write_file`, `edit_file` | ✅ | `WorkspaceContextProvider.add_tool_arc()` | scope 注入 + 审批包裹 + 路径守卫 |
| `list_files`, `inspect_file`, `make_directory` | ✅ | 同上 | 同上 |
| `remove_path`, `move_file` | ✅ | 同上 | 同上（含源+目标双路径检测） |
| `find_files`, `search_file` | ✅ | 同上 | 同上 |
| `run_command` | ✅ | 同上 | scope 注入 + 审批包裹 |
| `web_search`, `web_fetch` | ❌ | `AgentBuilder.with_tool()` | 普通注册，不受工作区约束 |
| `function` (自定义) | ❌ | `AgentBuilder.with_tool()` | 普通注册 |
| `custom` (工厂) | ❌ | `AgentBuilder.with_tool()` | 普通注册 |
| `mcp` | ❌ | `AgentBuilder.with_tool()` | 普通注册 |

> **为什么 IScopeTool 必须走 workspace 路径？** 只有通过 `WorkspaceContextProvider::add_tool_arc()`（内部调用 `try_inject_scope()` + `ApprovalRequiredTool` 包裹），工具才能获得 `WorkspaceScope` 引用并受 `ScopePolicy` 控制。直接注册到 `AgentBuilder` 的工具无法感知工作区边界。

### 两步处理：scope 注入 + 审批包裹

`WorkspaceContextProvider::add_tool_arc()` 对每个 IScopeTool 工具执行两步处理：

1. **Scope 注入**：通过 `AsAny` 下转型检测工具类型，调用 `IScopeTool::create_scoped(scope)` 注入 `WorkspaceScope`
2. **审批包裹**：若 `ScopePolicy::ApproveOutside`，用 `ApprovalRequiredTool` 包装工具

```rust
// add_tool_arc 内部逻辑（框架源码节选）
pub fn add_tool_arc(&mut self, tool: Arc<dyn ITool>) {
    let mut tool = tool;

    // Step 1: scope 注入（检测 IScopeTool）
    if let Some(scoped) = try_inject_scope(&tool, Arc::clone(&self.scope)) {
        tool = scoped;
    }

    // Step 2: 审批包裹（按策略）
    if self.scope.policy == ScopePolicy::ApproveOutside {
        tool = Arc::new(ApprovalRequiredTool::new(tool));
    }

    self.tools.push(tool);
}
```

## 加载代码

```rust
use rust_agent_decl::DeclAgentBuilder;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 一行代码：YAML → 可运行的 Agent
    let agent = DeclAgentBuilder::new()
        .from_yaml_file("agent.yaml")
        .build()
        .await?;

    // 运行 Agent（与手工构建的 Agent 完全相同）
    let session = agent.create_session();
    let messages = vec![rust_agent_core::ChatMessage::user(
        "读取 src/main.rs 的内容，并检查 /etc/hosts 是否存在"
    )];

    let mut stream = agent.run(messages, Some(session), None).await?;
    // ... 消费流式响应 ...
    Ok(())
}
```

## 策略行为对比

### AllowAll（无限制）

```yaml
contexts:
  - kind: workspace
    name: dev
    config:
      root: /home/dev/project
      policy: read                # read / allow / allow_all → AllowAll
```

- 工作区内外的所有文件操作直接执行
- 工具返回中 `scope` 字段为 `"workspace"` 或 `"outside_workspace"`
- 仅用于信息标注，不拒绝任何操作
- **适用场景**：本地开发、受信任环境

### ApproveOutside（推荐默认）

```yaml
contexts:
  - kind: workspace
    name: production
    config:
      root: /opt/myapp
      policy: approve             # approve / ask / approve_outside → ApproveOutside
```

- 工作区内操作直接执行
- 工作区外操作 → 触发 `ToolApprovalRequest` 等待用户审批
- 审批通过后执行，拒绝则返回错误
- **适用场景**：生产环境、需要审计的场景

### DenyOutside（严格限制）

```yaml
contexts:
  - kind: workspace
    name: sandbox
    config:
      root: /tmp/sandbox-42
      policy: deny                # deny / restrict / deny_outside → DenyOutside
```

- 工作区内操作直接执行
- 工作区外操作 → 工具直接返回 `ToolResult::error("Access denied")`
- LLM 看到错误后可调整行为或告知用户
- **适用场景**：沙箱环境、CI/CD、自动化测试

> **策略解析**：`DeclAgentBuilder` 通过 `ScopePolicy::from_config_str()` 解析 YAML 中的 `policy` 字段。支持的别名见 [8.1 工作区范围](./scope-overview.md#从配置字符串解析策略)。未知值会回退为 `DenyOutside`（安全优先，fail closed），并记录 `ERROR` 级别日志。

## 与 AgentBuilder 手工构建的对比

| 维度 | AgentBuilder 手工构建 | 声明式配置 |
|------|---------------------|-----------|
| 工作区定义 | `WorkspaceScope::new("/path", "name").with_policy(policy)` | YAML `contexts.workspace.config` |
| 工具注册 | 逐个 `.add_tool(ReadFile { scope: None })` | YAML `tools` 数组，一行一个工具 |
| Provider 构建 | `WorkspaceContextProvider::new(scope).add_tool(...)` | `DeclAgentBuilder` 自动构建 |
| 修改配置 | 需要改 Rust 代码并重新编译 | 只需改 YAML 文件 |
| 团队协作 | 需要 Rust 开发人员参与 | 非 Rust 开发人员也能修改配置 |
| 调试 | `println!` / 日志 | YAML 语法检查 + 日志输出 |

## 混合模式：YAML 声明 + 代码注入

声明式配置覆盖了大部分场景，但对于需要特殊处理的工作区（如动态路径、运行时计算），可以使用混合模式：

```rust
let agent = DeclAgentBuilder::new()
    .from_yaml_file("agent.yaml")                    // YAML 定义基础工作区
    .with_context({
        // 代码注入：运行时动态决定工作区路径
        let dynamic_root = std::env::var("PROJECT_ROOT")
            .unwrap_or_else(|_| "/default/path".into());
        let scope = WorkspaceScope::new(dynamic_root, "dynamic-workspace")
            .with_policy(ScopePolicy::ApproveOutside);
        Arc::new(WorkspaceContextProvider::new(Arc::new(scope))
            .add_tool(ReadFile::default())
            .add_tool(WriteFile::default()))
    })
    .build()
    .await?;
```

`with_context()` 注入的 provider 会与 YAML 中声明的 provider 合并，代码注入的 provider 排在后面，可以覆盖前面的行为。

## 故障排查

### 工作区配置未生效

检查 YAML 中 `policy` 字段的值是否正确：

```bash
# 查看 DeclAgentBuilder 日志输出
RUST_LOG=debug cargo run
```

输出中应包含类似的信息：
```
DEBUG rust_agent_decl: building workspace provider: root=/home/dev/myapp, policy=ApproveOutside
```

### 工具返回 scope 字段始终为 none

检查工具的 `scope` 字段是否正确初始化。声明式配置中的 `kind: file` 工具由 `ToolResolver` 创建时使用 `::default()`，`scope` 字段初始为 `None`。这意味着工具不会主动执行工作区裁剪——`path_guard.rs` 的 `resolve_safe()` 会根据 `base_dir` 参数判断。要启用完整的工作区感知，需配合 `WorkspaceContextProvider`（声明式 `contexts.workspace` 已自动处理这一点）。

## 下一步

- 了解路径守卫的底层实现 → [8.3 路径守卫与跨范围检测](path-guard.md)
- 了解跨范围审批的端到端流程 → [8.4 跨范围审批集成](cross-scope-approval.md)
- 查阅声明式配置的完整字段参考 → [10.5 配置字段完全参考](../10-macros-declarative/config-reference.md)

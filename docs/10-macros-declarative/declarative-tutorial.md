# 10.6 声明式 Agent 配置实战教程

## 概述

本教程手把手带你用 YAML 配置文件创建一个完整的、可直接投入生产的 Agent，涵盖模型配置、工具绑定、工作区管理、记忆系统和技能加载。学完本教程你将能够：

- 把现有的 `AgentBuilder` 代码迁移为声明式 YAML 配置
- 在 YAML 中配置多种类型的工具和上下文提供器
- 使用 `DeclAgentBuilder` 在 Rust 代码中一行加载配置文件
- 调试声明式配置中的常见问题

## 第一步：最简 Agent 的 YAML 文件

创建一个文件 `my-agent.yaml`：

```yaml
kind: prompt
name: hello-agent
model:
  id: agnes-2.0-flash
  provider: openai
  connection:
    kind: key
    api_key: $AGNES_API_KEY
instructions: 你是一个友好的助手，用中文简洁作答。
maxToolRounds: 3
```

Rust 加载代码：

```rust
use rust_agent_decl::DeclAgentBuilder;

let agent = DeclAgentBuilder::new()
    .from_yaml_file("my-agent.yaml")
    .build()
    .await?;

// 运行
let session = agent.create_session();
let messages = vec![ChatMessage::user("你好，请用一句话介绍自己。")];
let mut stream = agent.run(messages, Some(session), None).await?;
```

## 第二步：添加工具

在 YAML 中添加 `tools` 段。**内置工具的 description 已内建，无需手写**：

```yaml
kind: prompt
name: tool-agent
model:
  id: agnes-2.0-flash
  provider: openai
  connection:
    kind: key
    api_key: $AGNES_API_KEY
instructions: 你是文件管理助手，可以读写文件。
tools:
  # 内置工具：只需 kind + name
  - kind: file
    name: read_file
  - kind: file
    name: write_file
  - kind: file
    name: list_files

  # 自定义工具：必须提供 description
  - kind: function
    name: greet
    description: 用指定语言打招呼
maxToolRounds: 10
```

Rust 加载代码（需注册自定义工具工厂）：

```rust
use rust_agent_decl::DeclAgentBuilder;
use rust_agent_macros::tool;
use rust_agent_core::ToolResult;

// 定义 YAML 中声明的自定义工具
#[tool(description = "用指定语言打招呼")]
async fn greet(
    #[param(desc = "语言代码（如 zh, en）")] lang: String,
) -> ToolResult {
    let greeting = match lang.as_str() {
        "zh" => "你好！",
        "en" => "Hello!",
        _ => "👋",
    };
    ToolResult::success(serde_json::json!({"greeting": greeting}))
}

let agent = DeclAgentBuilder::new()
    .from_yaml_file("tool-agent.yaml")
    .with_tool("greet", |_| Ok(std::sync::Arc::new(greet_stub())))
    .build()
    .await?;
```

## 第三步：配置工作区

添加 `contexts` 段定义工作区边界：

```yaml
contexts:
  - kind: workspace
    name: project-root
    config:
      root: /home/dev/myapp        # 工作区根路径
      policy: approve               # 越界操作需审批
```

> 完整的工作区配置教程见 [第 8.5 节：声明式工作区配置与工具联动](../08-workspace-management/declarative-workspace.md)。

## 第四步：添加记忆系统

添加持久化记忆，让 Agent 在多次对话之间记住用户偏好：

```yaml
contexts:
  - kind: memory
    name: skill-memory
    config:
      directory: logs/memory
      enabled: true
      consolidationInterval: 1   # 每次对话后整理记忆
```

## 第五步：添加技能系统

加载 SKILL.md 文件作为 Agent 的领域知识：

```yaml
contexts:
  - kind: skills
    name: code-review
    config:
      directory: skills/code-review   # 技能目录路径
```

技能目录结构：

```
skills/code-review/
├── SKILL.md                 # 技能定义（必需）
├── references/
│   └── rust-guidelines.md   # 参考资料
└── assets/
    └── templates/           # 模板文件
```

## 最终完整配置

把所有内容组合起来：

```yaml
kind: prompt
name: production-agent
displayName: 生产级全栈助手
description: 具备工作区管理、记忆、技能和多种工具的全栈开发 Agent

model:
  id: agnes-2.0-flash
  provider: openai
  connection:
    kind: key
    api_key: $AGNES_API_KEY
    endpoint: https://apihub.agnes-ai.com/v1
  options:
    temperature: 0.3
    maxTokens: 8192
    topP: 0.95

instructions: |
  你是一个全栈开发助手。核心准则：

  1. **先理解再动手**：不确定就问，不要猜测
  2. **最小改动**：只写必要的代码，不过度设计
  3. **工作区感知**：所有文件操作在指定工作区内进行
  4. **记忆利用**：从持久记忆中检索用户偏好和项目上下文
  5. **技能驱动**：对代码审查、测试等任务使用专项技能

additionalInstructions: |
  当前项目技术栈：Rust 2021 edition + Axum + PostgreSQL。

# ── 上下文提供器 ──
contexts:
  - kind: workspace
    name: project-root
    config:
      root: /home/dev/project
      policy: approve
  - kind: memory
    name: skill-memory
    config:
      directory: logs/memory
      consolidationInterval: 1
  - kind: skills
    name: code-review
    config:
      directory: skills/code-review

# ── 工具 ──
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
  - kind: web
    name: web_search

maxToolRounds: 20
```

Rust 入口代码：

```rust
use rust_agent_decl::DeclAgentBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let agent = DeclAgentBuilder::new()
        .from_yaml_file("production-agent.yaml")
        .build()
        .await?;

    // 使用 ReplRunner 获得开箱即用的交互体验
    rust_agent_cli::ReplRunner::new(agent)
        .banner("🦀 全栈开发助手 — 输入 /help 查看命令")
        .run()
        .await?;

    Ok(())
}
```

## 调试技巧

### 1. 验证 YAML 语法

```bash
# 使用 Python 检查 YAML 语法
python3 -c "import yaml; yaml.safe_load(open('agent.yaml'))"

# 或使用 yamllint
yamllint agent.yaml
```

### 2. 查看解析后的结构

```rust
use rust_agent_decl::AgentDocument;

let doc = AgentDocument::from_yaml_file("agent.yaml")?;
let def = doc.inner_definition();
println!("{:#?}", def);
```

### 3. 启用调试日志

```bash
RUST_LOG=debug,rust_agent_decl=trace cargo run
```

### 4. 常见错误及解决

| 错误信息 | 原因 | 解决 |
|---------|------|------|
| `Unknown tool 'xxx'` | `function`/`custom` 工具未注册工厂 | 添加 `.with_tool("xxx", factory)` |
| `YAML feature is required` | 未启用 yaml feature | `features = ["yaml"]` |
| `AgentDocument contains a Manifest` | YAML 顶层有 `template:` 字段 | 去掉 Manifest 包装 |

## 从 AgentBuilder 迁移对照

| AgentBuilder 写法 | YAML 配置 |
|---|---|
| `AgentBuilder::new("my-agent")` | `name: my-agent` |
| `.instructions("你是助手")` | `instructions: 你是助手` |
| `.chat_client(DeepSeekChatClient::from_key(...))` | `model: { connection: { kind: key, api_key: $KEY } }` |
| `.with_tool(ReadFile { scope: None })` | `tools: [{ kind: file, name: read_file }]` |
| `.with_tool(Echo)` | `tools: [{ kind: function, name: echo, description: ... }]` |
| `.max_tool_rounds(15)` | `maxToolRounds: 15` |
| `.add_context_provider(prov)` | `contexts: [{ kind: ..., name: ..., config: {...} }]` |
| `WorkspaceScope::new("/path", "name")` | `contexts: [{ kind: workspace, config: { root: /path, policy: approve } }]` |

## 下一步

- 查阅完整的配置字段参考 → [10.5 配置字段完全参考](config-reference.md)
- 深入 AgentSchema v1.0 规范 → [10.4 AgentSchema v1.0 规范](agent-schema.md)
- 了解工作区配置的完整教程 → [8.5 声明式工作区配置与工具联动](../08-workspace-management/declarative-workspace.md)

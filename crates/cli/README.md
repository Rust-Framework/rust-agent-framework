# rust-agent-cli

基于 Rust Agent Framework (RAF) 的交互式命令行智能体。采用声明式 YAML 驱动 `DeclAgentBuilder` 构建 Agent，通过 `ReplRunner` 提供开箱即用的 REPL 体验。

## 架构概览

```
cli-agent.yaml (MAF v1.0) → DeclAgentBuilder → Arc<dyn IAgent> → ReplRunner → REPL
```

- **声明式配置**：`cli-agent.yaml` 定义模型、系统指令、工具清单，修改无需重编译
- **运行时注入**：API Key、SkillMemory、工具工厂在 `main.rs` 中通过 Builder 方法注入
- **开箱即用 REPL**：`ReplRunner` 封装 rustyline 循环、命令解析、流式渲染、Token 统计

## 快速启动

### 前置条件

- Rust 1.80+
- DeepSeek API Key（或兼容 OpenAI 的 API）
- 设置环境变量：

```bash
export AGNES_API_KEY="sk-your-key-here"
```

未设置时将使用编译期默认值（仅供开发测试）。

### 构建与运行

```bash
# 从工作区根目录启动
cargo run -p rust-agent-cli

# 指定日志级别
RUST_LOG=info cargo run -p rust-agent-cli
```

启动后进入 REPL 界面：

```
rust-agent-cli — Declarative Chat (DeepSeek)
Type /help for commands, /quit to exit.

> 你好，请介绍一下自己
[Agent 流式输出...]
```

## REPL 命令

| 命令 | 说明 |
|------|------|
| `/help` | 显示可用命令列表 |
| `/clear` | 清空对话历史 |
| `/restart` | 清空历史并重建 Agent（模拟新会话） |
| `/model <name>` | 运行时切换模型（如 `deepseek-chat`、`deepseek-reasoner`） |
| `/think on` | 启用深度思考模式（DeepSeek reasoning） |
| `/think off` | 关闭深度思考模式 |
| `/quit` / `/exit` | 退出程序（也支持不带斜杠的 `quit` / `exit`） |

## 声明式配置

Agent 的核心配置在 `cli-agent.yaml` 中维护，遵循 MAF AgentSchema v1.0 规范：

```yaml
kind: prompt
name: cli-agent
model:
  id: agnes-2.0-flash
  provider: deepseek
  connection:
    kind: key
    api_key: $AGNES_API_KEY          # 从环境变量读取
instructions: |
  You are a helpful AI assistant. Respond concisely.
  # ... 完整系统指令 ...
tools:
  - kind: web
    name: web_search
  - kind: web
    name: web_fetch
  - kind: function
    name: echo
  - kind: function
    name: add
max_tool_rounds: 8
```

修改 YAML 后无需重新编译，重启 CLI 即可生效。

## 工具清单

| 工具 | 来源 | 说明 |
|------|------|------|
| `web_search` | `rust-agent-websearch` | DuckDuckGo 网络搜索 |
| `web_fetch` | `rust-agent-websearch` | 网页内容抓取 |
| `echo` | `#[tool]` 宏 | 回显输入文本 |
| `add` | `#[tool]` 宏 | 两数相加 |

通过 `#[tool]` 宏定义的自定义工具需在 `main.rs` 中注册工厂闭包：

```rust
DeclAgentBuilder::new()
    .with_tool("echo", |_| Ok(Arc::new(Echo)))
    .with_tool("add", |_| Ok(Arc::new(Add)))
```

## 流式输出渲染

REPL 使用 ANSI 彩色格式实时呈现完整的工具调用生命周期：

| 内容类型 | 显示格式 |
|---------|---------|
| `Text` | 实时文本输出（打字机效果） |
| `Reasoning` | 灰色 `[思考]` 前缀 |
| `ToolCallStart` | 青色 `[调用] tool_name` |
| `ToolCallArgsParsed` | 绿色 `param = value` |
| `ToolCalling` | 黄色 `[参数] tool_name args_json` |
| `ToolCalled` | 绿色 `[结果]` / 红色 `[结果] 失败` |
| `Usage` | 灰色 `[用量] prompt=N cache=... completion=N total=N` |
| `Error` | 红色 `[错误] code: message` |

## 代码集成：ReplRunner 组件

`ReplRunner` 是开箱即用的 REPL 组件，可嵌入任意 RAF 项目：

```rust
use rust_agent_cli::ReplRunner;

// 最小方式：一行启动
ReplRunner::new(agent).run().await?;

// 完整方式：带模型切换和重启能力
ReplRunner::new(agent)
    .prompt("🦀 > ")
    .banner("My Chat — Type /help for commands")
    .on_switch_model(move |model| {
        Box::pin(async move {
            DeclAgentBuilder::new()
                .from_yaml_file("cli-agent.yaml")
                .with_model(&model)
                .build().await.map_err(Into::into)
        })
    })
    .on_restart(move || {
        Box::pin(async move {
            DeclAgentBuilder::new()
                .from_yaml_file("cli-agent.yaml")
                .build().await.map_err(Into::into)
        })
    })
    .run()
    .await?;
```

## 声明式 Agent 构建：DeclAgentBuilder

`DeclAgentBuilder` 位于 `rust-agent-decl` crate，提供与 `AgentBuilder` 一致的编码体验：

```rust
use rust_agent_decl::DeclAgentBuilder;

let agent = DeclAgentBuilder::new()
    .from_yaml_file("cli-agent.yaml")    // YAML 声明文件
    .with_model("agnes-2.0-flash")      // 可选：覆盖 YAML 中的模型
    .with_api_key(&api_key)               // 可选：覆盖 API Key
    .with_tool("echo", |_| Ok(Arc::new(Echo)))
    .with_context(skill_memory)           // 可选：注入 ContextProvider
    .build()
    .await?;
```

| 方法 | 说明 |
|------|------|
| `from_yaml_file(path)` | 从 YAML 文件加载声明 |
| `from_yaml_str(yaml)` | 从字符串加载声明 |
| `with_model(id)` | 覆盖 YAML 中的 model.id |
| `with_api_key(key)` | 设置 API Key（覆盖 `$VAR` 占位符） |
| `with_tool(name, factory)` | 注册工具工厂（YAML 中声明的自定义工具） |
| `with_context(provider)` | 注入 ContextProvider（如 SkillMemory） |
| `max_tool_rounds(n)` | 覆盖最大工具调用轮次 |
| `build()` | 构建，返回 `Arc<dyn IAgent>` |

## 架构图

```
┌─────────────────┐     ┌─────────────────────┐     ┌──────────────┐
│  cli-agent.yaml  │────▶│  DeclAgentBuilder    │────▶│  IAgent      │
│  (MAF v1.0)      │     │  + AgentResolver     │     │              │
│                  │     │  + ToolResolver      │     │              │
│  - model         │     │  + ConnectionResolver│     │              │
│  - instructions  │     │  + ContextProvider   │     │              │
│  - tools         │     └─────────────────────┘     └──────┬───────┘
└─────────────────┘                                        │
                                                           ▼
┌─────────────────┐     ┌─────────────────────┐     ┌──────────────┐
│  用户输入        │────▶│  ReplRunner          │◀────│  Agent       │
│  (REPL)         │     │                      │     │              │
│                  │     │  - /help /clear     │     │              │
│  - rustyline     │     │  - /model /restart  │     │              │
│  - 历史记录       │     │  - /think on/off    │     │              │
│                  │     │  - 流式渲染          │     │              │
└─────────────────┘     │  - Token 统计        │     └──────────────┘
                        └─────────────────────┘
```

## API Key 管理

支持三种方式，优先级从高到低：

1. **环境变量** `AGNES_API_KEY`（推荐生产环境）
2. **`DeclAgentBuilder::with_api_key()`** 运行时传入
3. **编译期默认值**（仅供开发测试，`main.rs:20`）

YAML 中的 `api_key: $AGNES_API_KEY` 在解析时自动展开环境变量（由 `ConnectionResolver` 处理）。

## 依赖

| Crate | 说明 |
|-------|------|
| `rust-agent-core` | 核心类型和 Session |
| `rust-agent-framework` | `AgentBuilder`、`tool` 宏、SkillMemory |
| `rust-agent-client` | `DeepSeekChatClient` |
| `rust-agent-decl` | `DeclAgentBuilder`、声明式配置 |
| `rust-agent-websearch` | `WebSearch` / `WebFetch` 工具 |
| `tokio` | 异步运行时 |
| `tracing-subscriber` | 日志（chat 模式下仅 warn 级别） |
| `rustyline` | readline 风格行编辑和历史 |

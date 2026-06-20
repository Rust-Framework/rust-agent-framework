# rust-agent-decl

基于开放标准的 AI Agent 声明式定义框架。通过 JSON / YAML / TOML 数据文件声明式构建和编排 AI Agent 与多 Agent 工作流。

## 设计理念

`rust-agent-decl` 遵循 **Agent 声明协议（Agent Declaration Protocol）**，一个开放的、格式无关的 Agent 数据描述规范。协议对齐以下行业标准：

| 概念 | 对齐标准 |
|------|----------|
| **工具定义** | [OpenAI Function Calling](https://platform.openai.com/docs/guides/function-calling) — `name` + `description` + `parameters`（JSON Schema） |
| **模型配置** | OpenAI 兼容的 Provider 设置 — `provider` + `model` + `api_key` |
| **参数 Schema** | [JSON Schema Draft-07](https://json-schema.org/) |
| **消息格式** | [OpenAI Chat Completions API](https://platform.openai.com/docs/api-reference/chat) — `system` / `user` / `assistant` / `tool` 角色 |
| **序列化格式** | JSON / YAML / TOML |

## 快速开始

### 扩展 Trait 风格（推荐）

```rust
use rust_agent_decl::AgentBuilderExt;
use rust_agent_framework::AgentBuilder;

let json = std::fs::read_to_string("agent.json")?;
let agent = AgentBuilder::from_json_decl(&json)?
    .with_tool(my_custom_tool)  // 可继续链式定制
    .build()?;
```

### Resolver 风格

```rust
use rust_agent_decl::{AgentDecl, DefaultAgentResolver};
use rust_agent_decl::resolver::AgentResolver;

let decl = AgentDecl::from_json_file("agent.json")?;
let resolver = DefaultAgentResolver::new();
let agent = resolver.resolve(&decl).await?;
```

## 数据协议

### AgentDecl — Agent 声明

一个完整的 Agent 定义遵循以下 JSON Schema：

```json
{
  "version": "1.0",
  "$schema": "https://example.com/agent-decl-1.0.json",
  "id": "code-reviewer",
  "description": "代码审查专家",
  "instructions": "你是一个资深代码审查专家...",
  "model": { ... },
  "tools": [ ... ],
  "contexts": [ ... ],
  "compression": { ... },
  "token_counter": { ... },
  "properties": { ... },
  "max_tool_rounds": 10,
  "run_options": { ... },
  "sub_agents": [ ... ]
}
```

#### 字段说明

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|:---:|--------|------|
| `version` | `string` | 否 | `"1.0"` | 协议版本号 |
| `$schema` | `string` | 否 | `""` | JSON Schema 引用 URI |
| `id` | `string` | **是** | — | Agent 唯一标识符 |
| `description` | `string` | 否 | `""` | 人类可读的描述 |
| `instructions` | `string` | 否 | `""` | 系统指令（System Prompt） |
| `model` | `object` | **是** | — | 模型配置（见 ModelConfig） |
| `tools` | `array` | 否 | `[]` | 工具引用列表（见 ToolRef） |
| `contexts` | `array` | 否 | `[]` | 上下文提供器列表（见 ContextProviderDecl） |
| `compression` | `object` | 否 | `null` | 压缩策略（见 CompressionDecl） |
| `token_counter` | `object` | 否 | `null` | Token 计数器（见 TokenCounterDecl） |
| `properties` | `object` | 否 | `{}` | 自定义键值属性 |
| `max_tool_rounds` | `number` | 否 | `10` | 最大工具调用轮次 |
| `compression` | `object` | 否 | `null` | 压缩策略（见 CompressionDecl，需 `token_counter` 或自动 estimate） |
| `token_counter` | `object` | 否 | `null` | Token 计数器（见 TokenCounterDecl） |
| `sandbox` | `object` | 否 | `{}` | 代码沙箱默认（`kind: code` 工具继承） |
| `run_options` | `object` | 否 | `null` | 运行参数覆盖（温度、max_tokens 等） |
| `sub_agents` | `array` | 否 | `[]` | 子 Agent 声明（递归解析） |

---

### ModelConfig — 模型配置

对齐 OpenAI Chat Completions API 的 provider 配置模式。

```json
{
  "provider": "openai",
  "model": "gpt-4o",
  "api_key": "$OPENAI_API_KEY",
  "base_url": "https://api.openai.com/v1",
  "temperature": 0.7,
  "max_tokens": 4096,
  "extra_headers": { "OpenAI-Organization": "org-xxx" },
  "extra": { "reasoning_effort": "high" }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|:---:|------|
| `provider` | `string` | **是** | 提供商：`"openai"` / `"deepseek"` / `"custom"` |
| `model` | `string` | **是** | 模型名称，如 `"gpt-4o"`、`"deepseek-chat"` |
| `api_key` | `string` | **是** | API 密钥。支持 `$ENV_VAR` 语法读取环境变量（如 `"$OPENAI_API_KEY"`） |
| `base_url` | `string` | 否 | API Base URL。`"custom"` provider 时必填 |
| `temperature` | `number` | 否 | 采样温度 (0.0–2.0) |
| `max_tokens` | `number` | 否 | 最大输出 token 数 |
| `extra_headers` | `object` | 否 | 额外 HTTP 请求头 |
| `extra` | `object` | 否 | 扩展配置（透传给 Provider） |

#### 支持的 Provider

| Provider | 默认 Base URL |
|----------|--------------|
| `openai` | `https://api.openai.com/v1` |
| `deepseek` | `https://api.deepseek.com` |
| `custom` | 需手动指定 `base_url` |

---

### ToolRef — 工具引用

对齐 OpenAI Function Calling 的工具定义模式。工具参数使用 **JSON Schema Draft-07** 描述。

工具类型通过 `type` 字段区分（tagged enum）：

#### 1. 内置工具 (`builtin`)

框架预置的 13 个工具，零配置开箱即用。可按分类批量注册（省略 `name` 即注册该分类全部工具）：

```yaml
# YAML 声明式
tools:
  - kind: web      # 无 name → 注册 web_search + web_fetch
  - kind: file     # 无 name → 注册全部 11 个文件工具
  - kind: web
    name: web_search   # 指定 name → 仅注册单个工具
```

```json
{ "type": "builtin", "name": "read_file" }
```

**内置工具列表**：

| 工具名 | 说明 |
|--------|------|
| `read_file` | 读取文件内容，支持行范围 |
| `write_file` | 写入文件 |
| `edit_file` | 编辑文件（old_str → new_str） |
| `list_files` | 列出目录内容 |
| `inspect_file` | 查看文件元信息 |
| `make_directory` | 创建目录 |
| `remove_path` | 删除文件或目录 |
| `move_file` | 移动 / 重命名文件 |
| `find_files` | 按 glob 模式查找文件 |
| `search_file` | 按内容搜索文件 |
| `run_command` | 执行系统命令 |
| `web_search` | 网页搜索 |
| `web_fetch` | 抓取网页内容 |

#### 2. Rhai 脚本工具 (`rhai`)

将 Rhai 脚本封装为可调用工具，参数 Schema 使用 JSON Schema 定义。

```json
{
  "type": "rhai",
  "name": "calculate",
  "description": "执行数学计算",
  "script_path": "./tools/calc.rhai",
  "parameters": {
    "type": "object",
    "properties": {
      "expression": { "type": "string", "description": "数学表达式" }
    },
    "required": ["expression"]
  }
}
```

#### 4. 代码沙箱工具 (`code`)

需启用 `sandbox` feature。声明 `kind: code` 自动构建 `code_interpreter`，无需手动 `with_tool`：

```yaml
tools:
  - kind: code
    name: code_interpreter
    config:
      backend: process          # process | container | docker | podman | wasm
      timeout_secs: 60
      default_language: python
```

Agent 级默认（子工具 config 可覆盖）：

```yaml
sandbox:
  backend: process
  timeout_secs: 30
```

#### 5. OpenAPI 工具 (`openapi`)

需启用 `openapi` feature；响应 JSON Schema 校验需 `openapi-validate`：

```yaml
tools:
  - kind: openapi
    name: get_pet
    specUrl: file://./petstore.yaml
    operationId: getPetById
```

#### 6. 自定义工具 (`custom`)

通过工厂函数注册的 Rust 原生工具（见下方"自定义工具"）。

```json
{ "type": "custom", "name": "weather_lookup" }
```

---

### ContextProviderDecl — 上下文提供器

通过 `(kind, name)` 二元组声明式配置，支持 6 种分类：

```yaml
contexts:
  - kind: memory
    name: skill-memory
    config:
      directory: logs/memory
      consolidationInterval: 1
  - kind: skills
    name: antd-skill
    config:
      directory: skills/antd-skill
  - kind: workspace
    name: default
    config:
      root: .
      policy: read
```

| kind | name 示例 | 说明 | 典型 config 键 |
|------|-----------|------|---------------|
| `memory` | `skill-memory` | 持久化跨会话记忆系统 | `directory`, `enabled`, `consolidationInterval` |
| `skills` | `antd-skill` | 按需加载的技能文件（SKILL.md） | `directory` |
| `mcp` | `mymcp-server` | MCP 远程工具服务器 | `serverUrl` |
| `workspace` | `default` | 工作区根目录 + 访问策略 | `root`, `policy` |
| `knowledge` | `my-rag` | RAG 知识库检索 | `source` |
| `wiki` | `my-wiki` | Wiki 知识库 | `source` |

> `history`（InMemoryHistoryProvider）由 AgentBuilder 内置自动注入，无需声明。
> `websearch` 属于工具（`tools → kind: web`），不在此处配置。

---

### CompressionDecl — 压缩策略

| 类型 | 参数 | 说明 |
|------|------|------|
| `sliding_window` | `window_size` (number) | 滑动窗口，保留最近 N 条消息 |
| `token_budget` | — | Token 预算压缩，自动裁剪超限消息 |

---

### TokenCounterDecl — Token 计数器

| 类型 | 说明 |
|------|------|
| `estimate` | 近似估算（默认） |

---

### WorkflowDecl — 工作流声明

有向图结构的多 Agent 编排定义。MAF `kind: workflow` 使用 ActionDecl DSL：

```yaml
kind: workflow
name: code-runner
sandbox:
  backend: process
  timeout_secs: 30
trigger:
  kind: OnConversationStart
  id: start
  actions:
    - kind: ExecuteCode
      id: run_py
      code: print("hello")
      language: python
      sandbox:
        backend: process
      output:
        result: Local.code_out
```

#### ExecuteCode 动作

| 字段 | 说明 |
|------|------|
| `code` | 待执行源码（或 Rhai 表达式引用） |
| `language` | 语言标识（如 `python`） |
| `sandbox` | 动作级沙箱配置（继承工作流 `sandbox:` 默认值） |
| `output.result` | 结果写入的工作流状态键 |

需启用 `sandbox` feature。

### WorkflowGraph — 图式工作流（legacy）

```yaml
name: "research-workflow"
nodes:
  - type: agent
    id: "researcher"
    agent_ref: "researcher-agent"
  - type: agent
    id: "writer"
    agent_ref: "writer-agent"
    is_output: true
  - type: rhai
    id: "validator"
    script_path: "./scripts/validate.rhai"
edges:
  - type: direct
    source: "researcher"
    target: "validator"
  - type: direct
    source: "validator"
    target: "writer"
start_node_id: "researcher"
output_node_ids: ["writer"]
```

#### 节点类型

| 类型 | 说明 |
|------|------|
| `agent` | Agent 节点 — `agent_ref` 引用已注册 Agent 或 `agent` 内联声明 |
| `function` | 纯函数节点 — `function_ref` 引用工厂注册的函数 |
| `rhai` | Rhai 脚本节点 — 内联或文件引用的 Rhai 脚本 |

#### 边类型

| 类型 | 说明 |
|------|------|
| `direct` | 直接边：`source` → `target` |
| `fan_out` | 扇出边：`source` → 所有 `targets` 并行执行 |
| `fan_in` | 扇入边：所有 `sources` 完成后 → `target` |

---

## 完整示例

### 示例 1：Weather Assistant（JSON + 自定义工具）

**agent.json：**

```json
{
  "id": "weather-assistant",
  "description": "天气助手",
  "instructions": "You are a helpful weather assistant. Use the weather_lookup tool to get current weather data.",
  "model": {
    "provider": "openai",
    "model": "gpt-4o-mini",
    "api_key": "$OPENAI_API_KEY"
  },
  "tools": [
    { "type": "builtin", "name": "web_search" },
    { "type": "custom", "name": "weather_lookup" }
  ],
  "max_tool_rounds": 5
}
```

**Rust 代码：**

```rust
use std::sync::Arc;
use rust_agent_core::ITool;
use rust_agent_decl::{AgentDecl, DefaultAgentResolver, AgentBuilderExt};
use rust_agent_decl::resolver::AgentResolver;

// 1. 定义自定义工具（调用外部天气 API）
struct WeatherTool;

#[async_trait::async_trait]
impl ITool for WeatherTool {
    fn name(&self) -> &str { "weather_lookup" }
    fn description(&self) -> &str { "Get current weather for a city." }
    fn kind(&self) -> &str { "function" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "description": "City name" }
            },
            "required": ["city"]
        })
    }
    async fn execute(&self, args: serde_json::Value) -> rust_agent_core::Result<String> {
        let city = args["city"].as_str().unwrap_or("Beijing");
        let url = format!("https://api.open-meteo.com/v1/forecast?...");
        let resp = reqwest::get(&url).await?.json::<serde_json::Value>().await?;
        Ok(resp.to_string())
    }
}

// 2. 注册自定义工具并构建 Agent
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut resolver = DefaultAgentResolver::new();
    resolver.register_tool_factory("weather_lookup", |_| Ok(Arc::new(WeatherTool)));

    let decl = AgentDecl::from_json_file("agent.json")?;
    let agent = resolver.resolve(&decl).await?;

    // 运行 Agent
    let messages = vec![ChatMessage::user("What's the weather in Tokyo?")];
    let session = agent.create_session();
    let mut stream = agent.run(messages, Some(session), None).await?;
    // ... 处理流式输出
    Ok(())
}
```

### 示例 2：Code Review Agent（纯声明 + 内置工具）

```json
{
  "id": "code-reviewer",
  "description": "代码审查 Agent",
  "instructions": "You are a senior code reviewer. Analyze code for bugs, style issues, and security vulnerabilities.",
  "model": {
    "provider": "openai",
    "model": "gpt-4o",
    "api_key": "$OPENAI_API_KEY",
    "temperature": 0.3
  },
  "tools": [
    { "type": "builtin", "name": "read_file" },
    { "type": "builtin", "name": "list_files" },
    { "type": "builtin", "name": "search_file" }
  ],
  "max_tool_rounds": 10
}
```

### 示例 3：多 Agent 研究工作流（YAML）

```yaml
name: "deep-research"
nodes:
  - type: agent
    id: "searcher"
    agent_ref: "search-agent"
  - type: agent
    id: "analyst"
    agent_ref: "analysis-agent"
  - type: agent
    id: "writer"
    agent_ref: "writer-agent"
    is_output: true
edges:
  - type: direct
    source: "searcher"
    target: "analyst"
  - type: direct
    source: "analyst"
    target: "writer"
start_node_id: "searcher"
output_node_ids: ["writer"]
```

---

## API 参考

### AgentDecl

| 方法 | 说明 |
|------|------|
| `from_json_str(s)` | 从 JSON 字符串解析 |
| `from_json_file(path)` | 从 JSON 文件加载 |
| `from_yaml_str(s)` | 从 YAML 字符串解析 * |
| `from_yaml_file(path)` | 从 YAML 文件加载 * |
| `from_toml_str(s)` | 从 TOML 字符串解析 * |
| `from_toml_file(path)` | 从 TOML 文件加载 * |
| `to_json_string()` | 序列化为 JSON 字符串 |
| `to_json_pretty()` | 序列化为格式化 JSON |
| `to_yaml_string()` | 序列化为 YAML 字符串 * |
| `to_toml_string()` | 序列化为 TOML 字符串 * |

`*` 需要启用对应 feature：`yaml`、`toml`

### WorkflowDecl

与 `AgentDecl` 相同的方法签名。

### AgentBuilderExt（Extension Trait）

```rust
use rust_agent_decl::AgentBuilderExt;

// 从声明文本直接构建 AgentBuilder，可继续链式定制
AgentBuilder::from_json_decl(json_str)?
    .with_tool(my_tool)
    .build()?;

AgentBuilder::from_yaml_decl(yaml_str)?;  // feature = "yaml"
AgentBuilder::from_toml_decl(toml_str)?;  // feature = "toml"
```

### AgentResolver

```rust
// 完整异步解析（支持 Rhai 工具）
let resolver = DefaultAgentResolver::new();
resolver.register_tool_factory("my_tool", |config| { ... });
let agent = resolver.resolve(&decl).await?;
```

### 快捷函数

```rust
let agent = rust_agent_decl::quick_agent("agent.json").await?;
let graph = rust_agent_decl::quick_workflow("workflow.yaml").await?;
```

---

## Feature Flags

| Feature | 默认 | 说明 |
|---------|:---:|------|
| `json` | **是** | JSON 序列化支持 |
| `yaml` | 否 | YAML 序列化支持（`serde_yaml`） |
| `toml` | 否 | TOML 序列化支持（`toml`） |
| `rhai` | 否 | Rhai 工作流条件/表达式（`rust-agent-rhai`） |
| `web` | 否 | Web 搜索工具（`rust-agent-websearch`） |
| `rag` | 否 | RAG 知识库上下文 |
| `wiki` | 否 | Wiki 知识库上下文 |
| `openapi` | 否 | OpenAPI HTTP 工具 |
| `openapi-validate` | 否 | OpenAPI 响应 JSON Schema 校验（依赖 `jsonschema`） |
| `sandbox` | 否 | 内置 `code_interpreter` 沙箱（process/container） |
| `sandbox-docker` | 否 | Docker/Podman 沙箱后端 |
| `sandbox-wasm` | 否 | WASM 沙箱后端 |
| `mustache` | 否 | Mustache 模板渲染 |

```toml
[dependencies]
rust-agent-decl = { version = "0.1", features = ["yaml", "rhai", "wiki", "openapi"] }
```

---

## 测试控制台

```sh
# 交互式 REPL
cargo run --example test_console

# 自动测试模式
cargo run --example test_console -- --auto

# 启用调试日志
RUST_LOG=debug cargo run --example test_console
```

### REPL 命令

| 命令 | 说明 |
|------|------|
| `/help` | 显示帮助 |
| `/quit` | 退出 |
| `/clear` | 清空会话历史 |
| `/agent` | 显示当前 Agent 声明 |
| `/tools` | 列出可用工具 |
| `/think on\|off` | 切换推理模式 |
| `/load <file>` | 从 JSON 声明文件加载 Agent |
| `/validate <file>` | 验证声明文件 |
| 其他输入 | 发送给 Agent 的聊天消息 |

**API Key 配置**：编辑 `examples/test_console.rs` 中的 `DEEPSEEK_API_KEY` 常量。

---

## 架构

```
数据层                  解析层                   运行时层
┌──────────────┐     ┌──────────────┐     ┌──────────────────┐
│ agent.json   │──▶  │ AgentDecl    │──▶  │ AgentBuilder     │
│ agent.yaml   │     │ (from_json)  │     │   .build()       │──▶ Arc<dyn IAgent>
│ agent.toml   │     │ (from_yaml)  │     └──────────────────┘
└──────────────┘     │ (from_toml)  │
                     └──────┬───────┘
                            │ AgentBuilderExt
                            │ .from_json_decl() ──▶ AgentBuilder<ClientWrapper>
                            │
                            │ AgentResolver
                            │ .resolve() ──▶ Arc<dyn IAgent>（完整异步解析）
                            │
┌──────────────┐     ┌──────────────┐     ┌──────────────────┐
│ wf.json      │──▶  │ WorkflowDecl │──▶  │ WorkflowBuilder  │
│ wf.yaml      │     │ (from_json)  │     │   .build()       │──▶ WorkflowGraph
│ wf.toml      │     │ (from_yaml)  │     └──────────────────┘
└──────────────┘     │ (from_toml)  │
                     └──────────────┘
```

- **声明层**：纯数据模型，可序列化/反序列化，独立于运行时
- **扩展 Trait 层**：为 `AgentBuilder` / `WorkflowBuilder` 添加声明式构建方法
- **解析器层**：将声明转换为运行时对象，支持同步（Extension Trait）和异步（Resolver）两种路径

---

## 许可

MIT

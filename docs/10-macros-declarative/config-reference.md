# 10.5 声明式配置完整字段参考

本文是 RAF 声明式配置的**权威字段手册**，覆盖所有可配置字段的类型、必选/可选、默认值和有效值。面向用 JSON / YAML / TOML 编写 Agent 配置文件的开发者。

> **注意**：声明式配置中多个组件之间存在自动联动关系。例如 `contexts` 有 `workspace` 时 IScopeTool 工具会自动路由到工作区管理。详见 [10.8 组件联动规则](integration-patterns.md)。

---

## 顶层字段：AgentDefinition

| 字段 | JSON/TOML | 类型 | 必填 | 默认值 | 说明 |
|------|-----------|------|:---:|--------|------|
| `kind` | — | string | **是** | — | Agent 类型：`"prompt"` / `"workflow"` / `"hosted"` |
| `name` | — | string | **是** | — | Agent 唯一标识 |
| `displayName` | `display_name` | string | 否 | `""` | UI 展示名称 |
| `description` | — | string | 否 | `""` | 能力与用途描述 |
| `metadata` | — | object | 否 | `{}` | 任意键值对（作者、版本、标签等） |
| `inputSchema` | `input_schema` | PropertySchema | 否 | `null` | 输入参数定义（用于模板渲染） |
| `outputSchema` | `output_schema` | PropertySchema | 否 | `null` | 输出格式定义 |
| `model` | — | Model | **是** (prompt) | — | LLM 模型配置 |
| `instructions` | — | string | 否 | `""` | 系统指令文本 |
| `additionalInstructions` | `additional_instructions` | string | 否 | `null` | 附加指令（追加到 instructions 之后） |
| `template` | — | Template | 否 | `null` | Mustache/Jinja2 模板配置 |
| `tools` | — | ToolDecl[] | 否 | `[]` | 工具声明列表 |
| `contexts` | — | ContextProviderDecl[] | 否 | `[]` | 上下文提供器声明列表 |
| `maxToolRounds` | `max_tool_rounds` | number | 否 | `10` | 最大工具调用轮数 |
| `compression` | — | CompressionDecl | 否 | `null` | 消息压缩策略（需 `tokenCounter` 或自动 estimate） |
| `tokenCounter` | `token_counter` | TokenCounterDecl | 否 | `null` | Token 计数器 |
| `sandbox` | — | object | 否 | `{}` | 代码沙箱默认（`kind: code` 继承） |
| `subAgents` | `sub_agents` | AgentDefinition[] | 否 | `[]` | 子 Agent 定义（递归） |

> **JSON/TOML 列注**：当 YAML 使用 `camelCase` 时，TOML 中使用 `snake_case`；JSON 保持 `camelCase`（与 MAF 兼容）。

---

## Model — 模型配置

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|:---:|--------|------|
| `model.id` | string | **是** | — | 模型 ID，如 `"agnes-2.0-flash"`、`"gpt-4o"` |
| `model.provider` | string | 否 | — | Provider 标识：`"openai"` / `"custom"` |
| `model.connection.kind` | string | 否 | — | 连接类型：`key` / `remote` / `reference` / `oauth` / `anonymous` |
| `model.connection.api_key` | string | 否 | — | API 密钥。支持 `$ENV_VAR` / `=Env.VAR` |
| `model.connection.name` | string | 否 | — | `reference` 连接的目标名称 |
| `model.connection.endpoint` | string | 否 | — | 远程/OAuth 端点 URL |
| `model.options` | object | 否 | `null` | 模型推理参数 |

### Model Options 子字段

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `temperature` | number | Provider 默认 | 采样温度 (0.0-2.0) |
| `maxTokens` / `max_tokens` | number | Provider 默认 | 最大输出 token 数 |
| `topP` / `top_p` | number | Provider 默认 | Nucleus sampling 阈值 |
| `frequencyPenalty` / `frequency_penalty` | number | 0 | 频率惩罚 |
| `presencePenalty` / `presence_penalty` | number | 0 | 存在惩罚 |
| `seed` | number | — | 随机种子（确定性输出） |
| `stop` | string[] | — | 停止词列表 |

### JSON 示例

```json
{
    "model": {
        "id": "agnes-2.0-flash",
        "provider": "openai",
        "connection": {
            "kind": "key",
            "api_key": "$AGNES_API_KEY",
            "endpoint": "https://apihub.agnes-ai.com/v1"
        },
        "options": {
            "temperature": 0.3,
            "maxTokens": 4096,
            "topP": 0.95
        }
    }
}
```

### YAML 示例

```yaml
model:
  id: agnes-2.0-flash
  provider: openai
  connection:
    kind: key
    api_key: $AGNES_API_KEY
    endpoint: https://apihub.agnes-ai.com/v1
  options:
    temperature: 0.3
    maxTokens: 4096
    topP: 0.95
```

### TOML 示例

```toml
[model]
id = "agnes-2.0-flash"
provider = "openai"

[model.connection]
kind = "key"
api_key = "$AGNES_API_KEY"
endpoint = "https://apihub.agnes-ai.com/v1"

[model.options]
temperature = 0.3
max_tokens = 4096
top_p = 0.95
```

---

## Tools — 工具声明

`tools` 是一个数组，每个元素是一个 tagged object，`kind` 字段决定后续字段的结构。

### 工具 kind 分类总览

| kind | description 是否必须 | 可用 name 列表 | 说明 |
|------|:---:|---|------|
| `function` | **必须在 YAML 中提供** | 自定义（通过 `with_tool()` 注册） | 用户注册的函数工具 |
| `custom` | **必须在 YAML 中提供** | 自定义（通过 `register_factory()` 注册） | 工厂注册的自定义工具 |
| `web` | 无需提供（宏内置） | `web_search`, `web_fetch` | Web 搜索/抓取 |
| `file` | 无需提供（宏内置） | 见下方 file 工具表 | 文件系统操作 |
| `code` | 无需提供（宏内置） | `code_interpreter` | 代码沙箱执行（需 `sandbox` feature） |
| `mcp` | 无需提供（MCP 服务器提供） | 自定义 | MCP 远程工具 |
| `openapi` | 无需提供（规范内嵌） | 自定义 | OpenAPI 3.x HTTP 工具 |

> **重要规则**：`web` / `file` / `code` 等内置工具的 `description` 由 `#[tool]` 宏在编译期硬编码，YAML 中无需写 `description`。`function` 和 `custom` 类别必须手写 `description`。

### kind: function

```yaml
tools:
  - kind: function
    name: echo
    description: 将输入文本原样返回
    parameters: # 可选：JSON Schema
      properties:
        - name: text
          kind: string
          description: 要回显的文本
          required: true
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|:---:|------|
| `name` | string | **是** | 工具名称，需与 `with_tool("name", ...)` 注册的键匹配 |
| `description` | string | **是** | LLM 可见的功能说明 |
| `parameters` | PropertySchema | 否 | JSON Schema 参数定义 |
| `bindings` | ToolBinding[] | 否 | inputSchema → 参数的绑定映射 |

### kind: custom

```yaml
tools:
  - kind: custom
    name: weather_lookup
    description: 查询指定城市的天气
    config:
      api_endpoint: "https://api.weather.com/v2"
      default_unit: "celsius"
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|:---:|------|
| `name` | string | **是** | 工具名称，需与 `register_factory("name", ...)` 注册的键匹配 |
| `description` | string | **是** | LLM 可见的功能说明 |
| `config` | object | 否 | 透传给工厂函数的任意配置 |

### kind: web — 可用的 name 值

| name | 说明 |
|------|------|
| `web_search` | 网页搜索，返回搜索结果摘要 |
| `web_fetch` | 抓取指定 URL 的网页内容 |

```yaml
tools:
  # 方式 1：指定单个工具
  - kind: web
    name: web_search
  # 方式 2：不指定 name 则注册全部 web 工具
  - kind: web  # 等价于 web_search + web_fetch
```

### kind: file — 可用的 name 值

| name | 说明 |
|------|------|
| `read_file` | 读取文件内容，支持行范围 |
| `write_file` | 创建或覆盖文件 |
| `edit_file` | 精确替换文件中的字符串 |
| `list_files` | 列出目录内容 |
| `inspect_file` | 检查文件元数据（大小、类型等） |
| `make_directory` | 创建目录（含父目录） |
| `remove_path` | 删除文件或空目录 |
| `move_file` | 移动或重命名文件/目录 |
| `find_files` | 按 glob 模式搜索文件 |
| `search_file` | 在文件内容中搜索正则表达式 |
| `run_command` | 执行 shell 命令 |

```yaml
tools:
  # 方式 1：指定单个工具
  - kind: file
    name: read_file
  - kind: file
    name: write_file
  # 方式 2：不指定 name 则注册全部 11 个文件工具
  - kind: file
```

### kind: code

需启用 `rust-agent-decl` 的 `sandbox` feature。无需 `with_tool()` 工厂即可自动解析：

```yaml
tools:
  - kind: code
    name: code_interpreter
    config:
      backend: process       # process | container | docker | podman | wasm
      timeout_secs: 60
      default_language: python
      cpus: "1.0"
      pids_limit: 128
```

Agent 级默认（工具 `config` 可覆盖）：

```yaml
sandbox:
  backend: process
  timeout_secs: 30
```

详见 [13.9 代码沙箱](../13-extensions/sandbox.md)。

### kind: openapi

需启用 `openapi` feature；响应 Schema 校验需 `openapi-validate`：

```yaml
tools:
  - kind: openapi
    name: get_pet
    specUrl: file://./petstore.yaml
    operationId: getPetById
```

详见 [13.10 OpenAPI 工具](../13-extensions/openapi.md)。

### kind: mcp

```yaml
tools:
  - kind: mcp
    name: mcp-filesystem-tool
    server_url: "stdio://filesystem-server"
    tool_name: read_file
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|:---:|------|
| `name` | string | **是** | 工具名称 |
| `server_url` | string | **是** | MCP 服务器的连接 URL |
| `tool_name` | string | 否 | 服务器暴露的工具名（默认同 name） |
| `approval_mode` | string | 否 | 审批模式 |

---

## Contexts — 上下文提供器声明

`contexts` 是一个数组，每个元素是一个 tagged object，`kind` 字段决定提供器类型。

### 分类总览

| kind | config 键 | 实现状态 | 说明 |
|------|-----------|:---:|------|
| `memory` | `directory`, `enabled`, `consolidationInterval` | ✅ 已实现 | 持久化跨会话记忆 |
| `skills` | `directory` | ✅ 已实现 | 按需加载的技能文件 |
| `workspace` | `root`, `policy` | ✅ 已实现 | 工作区边界定义 |
| `mcp` | `serverUrl`, `command`, `args` | ⚠️ 需代码注入 | MCP 工具服务器 |
| `knowledge` | `source` | ✅ 已实现 | RAG 知识库（需 `rag` feature） |
| `wiki` | `source` | ✅ 已实现 | Wiki 知识库（需 `wiki` feature） |

> `mcp` 需要异步连接，无法在配置文件解析阶段完成。请通过 `DeclAgentBuilder::with_context()` 注入预连接的 `McpContextProvider`。
>
> `history`（对话历史）由 AgentBuilder 内置自动注入，无需在 `contexts` 中声明。

### kind: memory

```yaml
contexts:
  - kind: memory
    name: skill-memory
    config:
      directory: logs/memory
      enabled: true
      consolidationInterval: 1
```

| config 键 | 类型 | 默认值 | 说明 |
|-----------|------|--------|------|
| `directory` | string | `"logs/memory"` | 记忆存储目录 |
| `enabled` | boolean | `true` | 是否启用记忆系统 |
| `consolidationInterval` | number | `3` | 每 N 次对话后触发记忆整理 |

### kind: skills

```yaml
contexts:
  - kind: skills
    name: code-review
    config:
      directory: skills/code-review
```

| config 键 | 类型 | 默认值 | 说明 |
|-----------|------|--------|------|
| `directory` | string | `"skills/{name}"` | 技能目录路径。若留空则使用 `skills/{name}` |

技能目录结构参见 [第 13 章：技能系统](../13-extensions/skills.md)。

### kind: workspace

```yaml
contexts:
  - kind: workspace
    name: default
    config:
      root: /home/user/project
      policy: approve
```

| config 键 | 类型 | 默认值 | 说明 |
|-----------|------|--------|------|
| `root` | string | `"."` | 工作区根目录（绝对路径或当前目录相对的路径） |
| `policy` | string | `"approve"` | 越界策略。未知值回退为 `DenyOutside`（fail closed），并记录 ERROR 日志 |

#### policy 值映射

| YAML policy 值 | 对应的 ScopePolicy | 行为 |
|---|---|---|
| `"read"` / `"allow"` / `"allow_all"` | `AllowAll` | 无限制，所有路径均可访问 |
| `"approve"` / `"ask"` / `"approve_outside"` | `ApproveOutside` | 工作区外操作需用户审批 |
| `"deny"` / `"restrict"` / `"deny_outside"` | `DenyOutside` | 禁止任何工作区外访问 |

### kind: mcp（需代码注入）

```yaml
contexts:
  - kind: mcp
    name: filesystem-server
    config:
      serverUrl: "stdio://filesystem-server"
      command: "npx"
      args:
        - "@modelcontextprotocol/server-filesystem"
        - "/workspace"
```

> MCP 上下文提供器需要异步连接服务端，无法在 YAML 解析阶段完成。请在代码中预先连接并注入：

```rust
use rust_agent_mcp::{McpServerClient, McpConnectionOptions, McpContextProvider};

let server = McpServerClient::connect(
    McpConnectionOptions::stdio("filesystem-server", vec!["/workspace"]),
).await?;
let mcp_ctx = McpContextProvider::new(server);

let agent = DeclAgentBuilder::new()
    .from_yaml_file("agent.yaml")
    .with_context(Arc::new(mcp_ctx))
    .build()
    .await?;
```

### kind: knowledge / wiki

启用 `rag` / `wiki` feature 后，声明式路径可自动构建上下文提供器：

```yaml
contexts:
  - kind: knowledge
    name: docs-rag
    config:
      source: ./docs
  - kind: wiki
    name: project-wiki
    config:
      source: ./wiki-repo
```

未启用对应 feature 时，仍可通过 `DeclAgentBuilder::with_context()` 注入自定义实现。

---

## Compression — 压缩策略（框架扩展）

| 字段 | 类型 | 说明 |
|------|------|------|
| `compression.kind` | string | `sliding_window` / `token_budget` |
| `compression.windowSize` | number | 滑动窗口保留条数（`sliding_window`） |
| `compression.toolResultEvictionThreshold` | number | 工具结果淘汰阈值 0–1（`token_budget`，可选） |
| `tokenCounter.kind` | string | 目前支持 `estimate` |

```yaml
compression:
  kind: sliding_window
  windowSize: 20
tokenCounter:
  kind: estimate
```

> 配置 `compression` 但未写 `tokenCounter` 时，自动使用 `EstimateCounter`。

---

## Workflow — ExecuteCode 动作

`kind: workflow` Agent 的 `trigger.actions` 支持 `ExecuteCode`（需 `sandbox` feature）：

| 字段 | 说明 |
|------|------|
| `code` | 源码字符串 |
| `language` | 如 `python` |
| `sandbox` | 动作级沙箱配置（继承顶层 `sandbox:`） |
| `output.result` | 结果写入的工作流状态键 |

```yaml
kind: workflow
name: runner
sandbox:
  backend: process
trigger:
  kind: OnConversationStart
  id: start
  actions:
    - kind: ExecuteCode
      id: run
      code: print("ok")
      language: python
      output:
        result: Local.stdout
```

---

## Template — 模板配置

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|:---:|--------|------|
| `format` | string | **是** | — | `"mustache"` 或 `"plain"` |
| `content` | string | 否 | — | 内联模板内容 |
| `source_path` | string | 否 | — | 模板文件路径（与 content 二选一） |

```yaml
template:
  format: mustache
  content: |
    ## 任务
    {{task}}

    ## 约束
    - 语言：{{language}}
    - 代码风格：{{code_style}}
```

---

## PropertySchema — 属性模式

用于 `inputSchema` 和 `outputSchema`，定义输入输出参数：

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `strict` | boolean | `false` | 是否严格模式（拒绝未声明的属性） |
| `properties` | Property[] | `[]` | 属性列表 |
| `examples` | object[] | `[]` | 示例值列表 |

### Property 字段

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|:---:|--------|------|
| `name` | string | **是** | — | 属性名 |
| `kind` | string | **是** | — | 类型：`"string"` / `"integer"` / `"float"` / `"boolean"` / `"array"` / `"object"` |
| `description` | string | **是** | — | 属性描述 |
| `required` | boolean | 否 | `false` | 是否必须提供 |
| `default` | any | 否 | — | 默认值 |
| `enumValues` / `enum_values` | any[] | 否 | `[]` | 允许的枚举值列表 |

```yaml
inputSchema:
  strict: false
  properties:
    - name: task
      kind: string
      description: 用户请求的任务描述
      required: true
    - name: language
      kind: string
      description: 目标编程语言
      required: false
      enumValues: [rust, python, typescript, go]
```

---

## 三种格式完整示例

### YAML

```yaml
kind: prompt
name: coding-assistant
displayName: 编程助手
description: 具备代码生成、审查和测试能力的全栈智能体
metadata:
  author: team-platform
  version: "1.2.0"
  tags: [coding, enterprise, rust]
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
  你是企业级软件工程师。遵循以下原则：
  1. 代码必须经过充分测试
  2. 遵循 SOLID 原则
  3. 提供详细文档
additionalInstructions: 当前项目使用 Rust 2021 edition。
template:
  format: mustache
  content: |
    ## 任务
    {{task}}
    ## 约束
    - 语言：{{language}}
tools:
  - kind: file
    name: read_file
  - kind: file
    name: write_file
  - kind: file
    name: run_command
  - kind: web
    name: web_search
  - kind: function
    name: echo
    description: 将输入文本原样返回
contexts:
  - kind: memory
    name: skill-memory
    config:
      directory: logs/memory
      consolidationInterval: 1
  - kind: skills
    name: code-review
    config:
      directory: skills/code-review
  - kind: workspace
    name: default
    config:
      root: /home/user/project
      policy: approve
maxToolRounds: 15
subAgents:
  - kind: prompt
    name: code-reviewer
    displayName: 代码审查员
model:
  id: agnes-2.0-flash
  provider: openai
  connection:
    kind: key
    api_key: $AGNES_API_KEY
    instructions: 你是资深代码审查员。
    tools:
      - kind: file
        name: read_file
    maxToolRounds: 5
```

### JSON

```json
{
    "kind": "prompt",
    "name": "coding-assistant",
    "displayName": "编程助手",
    "description": "具备代码生成、审查和测试能力的全栈智能体",
    "metadata": {
        "author": "team-platform",
        "version": "1.2.0",
        "tags": ["coding", "enterprise", "rust"]
    },
    "model": {
        "id": "agnes-2.0-flash",
        "provider": "openai",
        "connection": {
            "kind": "key",
            "api_key": "$AGNES_API_KEY",
            "endpoint": "https://apihub.agnes-ai.com/v1"
        },
        "options": {
            "temperature": 0.3,
            "maxTokens": 8192,
            "topP": 0.95
        }
    },
    "instructions": "你是企业级软件工程师。遵循 SOLID 原则，提供详细文档。",
    "additionalInstructions": "当前项目使用 Rust 2021 edition。",
    "tools": [
        { "kind": "file", "name": "read_file" },
        { "kind": "file", "name": "write_file" },
        { "kind": "file", "name": "run_command" },
        { "kind": "web", "name": "web_search" },
        { "kind": "function", "name": "echo", "description": "将输入文本原样返回" }
    ],
    "contexts": [
        {
            "kind": "memory",
            "name": "skill-memory",
            "config": {
                "directory": "logs/memory",
                "consolidationInterval": 1
            }
        },
        {
            "kind": "workspace",
            "name": "default",
            "config": {
                "root": "/home/user/project",
                "policy": "approve"
            }
        }
    ],
    "maxToolRounds": 15
}
```

### TOML

```toml
kind = "prompt"
name = "coding-assistant"
display_name = "编程助手"
description = "具备代码生成、审查和测试能力的全栈智能体"

[metadata]
author = "team-platform"
version = "1.2.0"

[model]
id = "agnes-2.0-flash"
provider = "openai"

[model.connection]
kind = "key"
api_key = "$AGNES_API_KEY"
endpoint = "https://apihub.agnes-ai.com/v1"

[model.options]
temperature = 0.3
max_tokens = 8192
top_p = 0.95

instructions = """
你是企业级软件工程师。遵循 SOLID 原则，提供详细文档。
"""
additional_instructions = "当前项目使用 Rust 2021 edition。"

[[tools]]
kind = "file"
name = "read_file"

[[tools]]
kind = "file"
name = "write_file"

[[tools]]
kind = "web"
name = "web_search"

[[tools]]
kind = "function"
name = "echo"
description = "将输入文本原样返回"

[[contexts]]
kind = "memory"
name = "skill-memory"

[contexts.config]
directory = "logs/memory"
consolidation_interval = 1

[[contexts]]
kind = "workspace"
name = "default"

[contexts.config]
root = "/home/user/project"
policy = "approve"

max_tool_rounds = 15
```

> **注意**：TOML 格式使用 `snake_case` 命名（如 `display_name`、`additional_instructions`、`max_tool_rounds`），与 JSON/YAML 中的 `camelCase` 不同。

---

## 下一步

阅读完本参考手册后：
- 想了解 AgentSchema v1.0 规范全貌 → [10.4 AgentSchema v1.0 规范](agent-schema.md)
- 想了解 `#[tool]` 宏的代码生成细节 → [10.1 #[tool] 属性宏详解](tool-macro.md)
- 想了解声明式配置的整体架构 → [10.3 声明式 Agent/Workflow 配置](declarative-config.md)

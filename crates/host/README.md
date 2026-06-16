# rust-agent-host

基于官方 [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) v0.14 Rust SDK 实现的智能体主机服务端。桥接 [Rust Agent Framework (RAF)](https://gitcode.com/rf2026/rust-agent-framework) 多智能体框架，通过 JSON-RPC 2.0 向 ACP 兼容客户端（如基于 Rust + GPUI 开发的 AI 产品）提供智能体服务。

## 目录

- [架构概览](#架构概览)
- [快速开始](#快速开始)
- [配置指南](#配置指南)
- [内置智能体](#内置智能体)
- [ACP 协议交互流程](#acp-协议交互流程)
- [多智能体编排](#多智能体编排)
- [扩展方法](#扩展方法--_raf)
- [带标签流式输出](#带标签流式输出)
- [客户端集成指南](#客户端集成指南)
- [声明式智能体加载](#声明式智能体加载)
- [目录结构](#目录结构)
- [设计决策](#设计决策)

## 架构概览

### 系统架构

```
┌──────────────────────────┐                        ┌──────────────────────────┐
│      GPUI 客户端          │     ACP/JSON-RPC 2.0    │     rust-agent-host      │
│                          │◄───────────────────────►│                          │
│  ┌────────────────────┐  │   Stdio (子进程模式)     │  ┌────────────────────┐  │
│  │  多智能体视图        │  │   WebSocket (远程模式)   │  │  AcpAgentHandler   │  │
│  │  ┌──────┬──────┐    │  │                        │  │  (ACP SDK 桥接)     │  │
│  │  │子代理A│子代理B│   │  │                        │  └─────────┬──────────┘  │
│  │  │流式输出│流式输出│  │  │                        │            │             │
│  │  └──────┴──────┘    │  │                        │  ┌─────────▼──────────┐  │
│  └────────────────────┘  │                        │  │    桥接层           │  │
└──────────────────────────┘                        │  │ SessionMap          │  │
                                                    │  │ SubAgentMap         │  │
                                                    │  │ TypeConverter       │  │
                                                    │  └─────────┬──────────┘  │
                                                    │            │             │
                                                    │  ┌─────────▼──────────┐  │
                                                    │  │  AgentRegistry     │  │
                                                    │  │  多智能体注册/发现   │  │
                                                    │  └─────────┬──────────┘  │
                                                    │            │             │
                                                    │  ┌─────────▼──────────┐  │
                                                    │  │  rust-agent-       │  │
                                                    │  │  framework (RAF)   │  │
                                                    │  │  ChatClientAgent   │  │
                                                    │  │  WorkflowAsAgent   │  │
                                                    │  │  内置工具集         │  │
                                                    │  └────────────────────┘  │
                                                    └──────────────────────────┘
```

### 协议层依赖

| 层级 | 组件 | 说明 |
|------|------|------|
| 传输层 | Stdio / WebSocket (axum) | 本地子进程（标准 ACP）或远程部署 |
| 协议层 | `agent-client-protocol` v0.14 | 官方 Rust SDK，JSON-RPC 2.0 |
| 桥接层 | SessionBridge / TypeConverter | ACP ↔ RAF 类型转换与会话映射 |
| 服务层 | AgentRegistry / AgentFactory / DeclLoader | 多智能体注册、发现、创建 |
| 引擎层 | `rust-agent-framework` | ChatClientAgent、Workflow、内置工具 |

## 快速开始

### 启动服务

```bash
# Stdio 模式（标准 ACP，客户端作为子进程 spawn）
cargo run -p rust-agent-host -- --mode stdio

# WebSocket 模式（远程部署/独立服务）
cargo run -p rust-agent-host -- --mode ws --bind 127.0.0.1:9876

# 使用自定义配置文件和声明式智能体目录
cargo run -p rust-agent-host -- --mode ws --config host.toml --agents-dir ./agents
```

### 环境要求

- Rust 1.80+
- DeepSeek API Key（或其他 OpenAI 兼容提供商）

### CLI 参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--mode` | 传输模式：`stdio` 或 `ws` | `stdio` |
| `--bind` | WebSocket 监听地址 | `127.0.0.1:9876` |
| `--config` | TOML 配置文件路径 | `host.toml` |
| `--agents-dir` | 声明式智能体文件目录 | 无 |
| `--provider` | LLM 提供商 | `deepseek` |
| `--model` | 模型名称 | `deepseek-v4-flash` |
| `--api-key` | API 密钥（支持 `$ENV_VAR` 语法） | 无 |

### 环境变量

配置可通过 `RAF_` 前缀环境变量覆盖：

```bash
export RAF_PROVIDER__PROVIDER=deepseek
export RAF_PROVIDER__MODEL=deepseek-v4-flash
export RAF_PROVIDER__API_KEY=$DEEPSEEK_API_KEY
```

> 注：使用双下划线 `__` 分隔嵌套字段。

## 配置指南

### TOML 配置文件 (`host.toml`)

```toml
# 传输模式
mode = "ws"
ws_bind = "0.0.0.0:9876"

# LLM 提供商
[provider]
provider = "deepseek"
model = "deepseek-v4-flash"
api_key = "$DEEPSEEK_API_KEY"
temperature = 0.7

# 内置智能体开关
[agents]
coding = true
general = true
analysis = true

# 声明式智能体加载目录
agents_dir = "./agents"
```

### 配置优先级

配置加载遵循分层优先级（高到低）：

1. **CLI 参数** — 命令行直接指定
2. **环境变量** — `RAF_` 前缀的环境变量
3. **TOML 配置文件** — `host.toml`（或 `--config` 指定路径）
4. **代码默认值** — 硬编码合理默认值

### 提供商配置

支持任意 OpenAI 兼容的 API 提供商：

```toml
# DeepSeek（默认）
[provider]
provider = "deepseek"
model = "deepseek-v4-flash"
api_key = "$DEEPSEEK_API_KEY"

# OpenAI
[provider]
provider = "openai"
model = "gpt-4o"
api_key = "$OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"

# 自定义（兼容 OpenAI API 格式的第三方服务）
[provider]
provider = "openai"
model = "custom-model"
api_key = "your-api-key"
base_url = "https://your-api-endpoint.com/v1"
```

## 内置智能体

服务启动时自动注册以下三个预设智能体：

| 智能体 | ID | 专长 | 注册工具 | 说明 |
|--------|----|------|---------|------|
| **CodingAgent** | `coding` | 代码生成、审查、调试、重构 | ReadFile, WriteFile, EditFile, ListFiles, SearchFile, FindFiles, RunCommand, WebSearch, WebFetch | 15 轮工具调用上限 |
| **GeneralAgent** | `general` | 通用问答、写作、分析、创意 | WebSearch, WebFetch | 5 轮工具调用上限 |
| **AnalysisAgent** | `analysis` | 深度研究、多源对比、趋势分析 | WebSearch, WebFetch, ReadFile | 10 轮工具调用上限 |

### 智能体选择

客户端通过 `session/new` 的 `_meta` 字段指定目标智能体：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "session/new",
  "params": {
    "_meta": {
      "raf.agent_id": "coding"
    }
  }
}
```

不指定 `raf.agent_id` 时，自动使用默认智能体（第一个注册的）。

## ACP 协议交互流程

### 基本对话回合

```
客户端                              服务端                              智能体
  │                                  │                                   │
  │── initialize ──────────────────►│                                   │
  │◄── capabilities + agent list ──│                                   │
  │                                  │                                   │
  │── session/new ─────────────────►│                                   │
  │◄── sessionId ──────────────────│                                   │
  │                                  │                                   │
  │── session/prompt ──────────────►│                                   │
  │                                  │── IAgent::run(messages, opts) ──►│
  │                                  │                                   │── LLM 调用
  │                                  │◄── BoxStream<AgentResponseResult>│
  │◄── session/update ─────────────│                                   │
  │    (agent_message_chunk)        │                                   │
  │◄── session/update ─────────────│                                   │
  │    (tool_call: pending)         │                                   │
  │◄── session/update ─────────────│                                   │
  │    (tool_call: completed)       │                                   │
  │◄── session/update ─────────────│                                   │
  │    (agent_message_chunk)        │                                   │
  │◄── session/prompt response ────│                                   │
  │    (stopReason: end_turn)       │                                   │
```

### 消息格式参考

#### `initialize` 请求

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "method": "initialize",
  "params": {
    "protocolVersion": 1,
    "clientCapabilities": {}
  }
}
```

#### `initialize` 响应（含 RAF 智能体列表）

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "protocolVersion": 1,
    "agentCapabilities": {
      "prompt": { "text": true },
      "session": {},
      "mcp": {}
    },
    "_meta": {
      "raf": {
        "version": "0.1.0",
        "agents": [
          {
            "id": "coding",
            "agent_type": "ChatClientAgent",
            "name": "coding",
            "description": "代码专家智能体 — 代码生成、审查、调试、重构",
            "tool_names": ["read_file", "write_file", "edit_file", "..."],
            "capability_tags": [],
            "has_subagents": false,
            "is_default": false
          },
          {
            "id": "general",
            "is_default": true
          },
          {
            "id": "analysis",
            "is_default": false
          }
        ]
      }
    }
  }
}
```

#### `session/prompt` 请求

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "session/prompt",
  "params": {
    "sessionId": "sess_abc123def456",
    "prompt": [
      {
        "type": "text",
        "text": "分析这段代码的性能问题"
      },
      {
        "type": "resource",
        "resource": {
          "uri": "file:///home/user/main.py",
          "mimeType": "text/x-python",
          "text": "def process_data(items):\n    for item in items:\n        print(item)"
        }
      }
    ]
  }
}
```

#### `session/update` 通知（文本流式）

```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess_abc123def456",
    "update": {
      "sessionUpdate": "agent_message_chunk",
      "content": {
        "type": "text",
        "text": "我来分析这段代码的性能问题..."
      }
    }
  }
}
```

#### `session/update` 通知（工具调用）

```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess_abc123def456",
    "update": {
      "sessionUpdate": "tool_call",
      "toolCallId": "call_001",
      "title": "ReadFile",
      "kind": "other",
      "status": "pending"
    }
  }
}
```

#### `session/update` 通知（工具完成）

```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess_abc123def456",
    "update": {
      "sessionUpdate": "tool_call_update",
      "toolCallId": "call_001",
      "status": "completed",
      "content": [
        {
          "type": "content",
          "content": {
            "type": "text",
            "text": "文件内容:\ndef hello():\n    print('hello')"
          }
        }
      ]
    }
  }
}
```

#### `session/cancel` 通知

```json
{
  "jsonrpc": "2.0",
  "method": "session/cancel",
  "params": {
    "sessionId": "sess_abc123def456"
  }
}
```

## 多智能体编排

RAF 的核心多智能体能力通过 `get_subagent(agent_id)` 暴露子代理接口。ACP 对接采用三层模型：

### 三层对接模型

```
┌─────────────────────────────────────────────────────────────────┐
│                     多智能体 ACP 对接三层模型                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  第 1 层：发现 (Discovery)                                       │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ _raf/agent_list   → 获取所有顶级智能体列表                  │   │
│  │ _raf/subagent_list → 获取指定智能体的子代理列表             │   │
│  │                         (递归: get_subagent 遍历)         │   │
│  └─────────────────────────────────────────────────────────┘   │
│                           ↓                                     │
│  第 2 层：独立执行 (Direct Sessions)                             │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ session/new {_meta: {raf.agent_id}} → 创建子代理专属会话   │   │
│  │ session/prompt → 子代理独立流式输出                        │   │
│  │ 客户端可同时持有多个子代理会话，每个独立产生 session/update │   │
│  └─────────────────────────────────────────────────────────┘   │
│                           ↓                                     │
│  第 3 层：编排执行 (Orchestrated Sessions with Tagged Streaming) │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ session/new → 创建编排智能体会话                          │   │
│  │ session/prompt → 父代理执行，子代理自动调用               │   │
│  │ session/update {_meta: {raf.agent_id, raf.status}}       │   │
│  │ 每个更新标记来源子代理，客户端可分组展示                   │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### 模式 A：独立子代理会话（并行流式 + 多视图）

```
客户端                            服务端                        子代理A            子代理B
  │                                │                              │                  │
  │── _raf/subagent_list ────────►│                              │                  │
  │◄── [sub-A, sub-B] ───────────│                              │                  │
  │                                │                              │                  │
  │── session/new {agent:A} ─────►│                              │                  │
  │◄── sessionId_sessA ──────────│                              │                  │
  │── session/new {agent:B} ─────►│                              │                  │
  │◄── sessionId_sessB ──────────│                              │                  │
  │                                │                              │                  │
  ║                                ║                              ║                  ║
  ║  并行执行                      ║                              ║                  ║
  ║                                ║                              ║                  ║
  │── session/prompt(sessA) ──────►│── IAgent::run() ────────────►│                  │
  │                                │◄── stream ──────────────────│                  │
  │◄── session/update(sessA) ─────│  (_meta: agent_id="sub-A")   │                  │
  │◄── session/update(sessA) ─────│  (更多流式输出...)             │                  │
  │◄── prompt response(sessA) ────│                              │                  │
  ║                                ║                              ║                  ║
  ║                                ║                              ║                  ║
  │── session/prompt(sessB) ──────►│── IAgent::run() ─────────────────────────────►│
  │                                │◄── stream ───────────────────────────────────│
  │◄── session/update(sessB) ─────│  (_meta: agent_id="sub-B")                    │
  │◄── prompt response(sessB) ────│                                               │
  ║                                ║                              ║                  ║

  → 客户端同时渲染两个子代理视图，每个视图独立展示各自流式输出
```

### 模式 B：编排会话（带标签流式）

```
客户端                            服务端                        父代理(Workflow)    子代理A
  │                                │                              │                  │
  │── session/new {agent:parent} ─►│                              │                  │
  │◄── sessionId_parent ──────────│                              │                  │
  │── session/prompt(parent) ─────►│                              │                  │
  │                                │── IAgent::run() ────────────►│                  │
  │                                │                              │── 调度子代理A ──►│
  │                                │                              │◄── stream ──────│
  │◄── session/update(parent) ────│  (_meta: agent_id="sub-A",    │                  │
  │                                │         status="executing")  │                  │
  │◄── session/update(parent) ────│  (_meta: agent_id="sub-A",    │                  │
  │                                │         status="completed")  │                  │
  │◄── prompt response(parent) ───│                              │                  │
```

## 扩展方法 (`_raf/*`)

所有 RAF 特有功能通过 ACP 的扩展机制暴露。方法名以 `_` 前缀命名，遵循 [ACP 扩展性规范](https://agentclientprotocol.com/protocol/v1/extensibility)。

| 方法 | 参数 | 返回 | 说明 |
|------|------|------|------|
| `_raf/agent_list` | 无 | `{ agents: AgentInfo[] }` | 获取所有已注册智能体的完整元数据 |
| `_raf/agent_info` | `{ agent_id: string }` | `{ agent: AgentInfo }` | 查询指定智能体详情 |
| `_raf/subagent_list` | `{ agent_id: string }` | `{ agents: SubAgentInfo[] }` | 递归获取子代理列表 |
| `_raf/subagent_tree` | `{ agent_id: string }` | `{ tree: SubAgentNode }` | 获取完整子代理树结构 |

### AgentInfo 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 智能体唯一标识 |
| `agent_type` | string | 类型：`ChatClientAgent`、`WorkflowAgent` |
| `name` | string | 显示名称 |
| `description` | string | 功能描述 |
| `tool_names` | string[] | 注册工具名称列表 |
| `model_id` | string? | 使用的模型 ID |
| `capability_tags` | string[] | 能力标签 |
| `has_subagents` | bool | 是否含有子代理 |
| `is_default` | bool | 是否为默认智能体 |

### SubAgentInfo 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 子代理唯一标识 |
| `name` | string | 显示名称 |
| `agent_type` | string | 类型 |
| `description` | string | 功能描述 |
| `capability_tags` | string[] | 能力标签 |
| `depth` | usize | 在代理树中的深度 |
| `has_subagents` | bool | 是否含有下一级子代理 |

## 带标签流式输出

编排模式下的每个 `session/update` 通知通过 `_meta` 字段携带来源智能体信息：

```json
{
  "jsonrpc": "2.0",
  "method": "session/update",
  "params": {
    "sessionId": "sess_parent_001",
    "update": {
      "sessionUpdate": "agent_message_chunk",
      "content": {
        "type": "text",
        "text": "def fibonacci(n):\n    if n <= 1:\n        return n\n    ..."
      }
    },
    "_meta": {
      "raf.agent_id": "code-expert",
      "raf.agent_type": "ChatClientAgent",
      "raf.status": "executing"
    }
  }
}
```

### `_meta` 标签字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `raf.agent_id` | string | 产生此内容的智能体 ID |
| `raf.agent_type` | string | 智能体类型 |
| `raf.status` | string | 执行状态：`executing`、`completed`、`error` |
| `raf.elapsed_ms` | number? | 子代理执行耗时（毫秒），仅在 `completed` 状态时出现 |

### RAF → ACP 输出映射

| RAF 输出 | ACP SessionUpdate | `_meta.raf` 标签 |
|----------|-------------------|------------------|
| `Content::Text` | `agent_message_chunk` | `{agent_id, status: "executing"}` |
| `Content::Reasoning` | `agent_message_chunk`（role=thought） | `{agent_id}` |
| `Content::ToolCallStart` | `tool_call`（status=pending） | `{agent_id}` |
| `Content::ToolCallArgs` | `tool_call_update`（status=in_progress） | `{agent_id}` |
| `Content::ToolCalled` | `tool_call_update`（status=completed） | `{agent_id}` |
| `Content::Usage` | `usage_update` | `{agent_id}` |
| 子代理启动 | `agent_message_chunk`（空内容） | `{agent_id, status: "executing"}` |
| 子代理完成 | `agent_message_chunk`（空内容） | `{agent_id, status: "completed", elapsed_ms}` |
| 子代理错误 | `agent_message_chunk`（错误内容） | `{agent_id, status: "error"}` |

## 客户端集成指南

### GPUI 客户端集成流程

#### 1. Stdio 模式（本地子进程）

客户端通过标准子进程启动 `rust-agent-host`，通过 stdin/stdout 进行 JSON-RPC 通信：

```rust
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};

// 启动 rust-agent-host 子进程
let mut child = Command::new("rust-agent-host")
    .arg("--mode").arg("stdio")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()?;

let mut stdin = child.stdin.take().unwrap();
let stdout = child.stdout.take().unwrap();
let reader = BufReader::new(stdout);

// 发送 initialize 请求
let init = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1}}"#;
writeln!(stdin, "{}", init)?;

// 读取响应行
for line in reader.lines() {
    let line = line?;
    let msg: serde_json::Value = serde_json::from_str(&line)?;
    // 处理消息...
}
```

#### 2. WebSocket 模式（远程部署）

客户端通过 WebSocket 连接到服务端：

```rust
use tokio_tungstenite::connect_async;
use futures_util::StreamExt;

let (ws_stream, _) = connect_async("ws://127.0.0.1:9876/acp").await?;
let (mut write, mut read) = ws_stream.split();

// 发送 JSON-RPC 消息（每行一个完整的 JSON）
let msg = serde_json::json!({
    "jsonrpc": "2.0",
    "id": 0,
    "method": "initialize",
    "params": { "protocolVersion": 1 }
});
write.send(Message::Text(msg.to_string())).await?;

// 接收响应
while let Some(msg) = read.next().await {
    match msg {
        Ok(Message::Text(text)) => {
            let response: serde_json::Value = serde_json::from_str(&text)?;
            // 处理响应...
        }
        _ => {}
    }
}
```

### 实现多代理视图的步骤

客户端通过以下步骤实现多智能体并行展示：

1. **连接后**：调用 `initialize` → 从响应的 `_meta.raf.agents[]` 获取智能体列表
2. **探索子代理**：对编排智能体调用 `_raf/subagent_list` 或 `_raf/subagent_tree`
3. **选择执行模式**：

   **直接模式**（每个子代理独立会话）：
   - 对每个想要观看的子代理调用 `session/new {_meta: {raf.agent_id}}`
   - 各自调用 `session/prompt`，独立消费 `session/update`
   - GPUI 渲染多个 `AgentView` 组件，每个订阅各自的 `session/update` 流

   **编排模式**（父代理统一管理）：
   - 对编排智能体调用 `session/new {_meta: {raf.agent_id: "workflow"}}`
   - 调用 `session/prompt`
   - 按 `_meta.raf.agent_id` 分组展示输出，按 `_meta.raf.status` 更新进度

### 客户端多视图渲染示意

```
┌─────────────────────────────────────────────────────────────┐
│  RAF Agent Host — 多智能体视图                                │
├───────────────────┬───────────────────┬─────────────────────┤
│  父代理视角         │  代码专家 (executing) │  测试专家 (pending)    │
│                   │                   │                     │
│  任务：构建 Web 服务器│  def fibonacci(n): │  [等待代码专家完成...]   │
│  → 分配任务给代码专家 │      if n <= 1:   │                     │
│  → 等待测试专家反馈   │          return n  │                     │
│                   │      ...           │                     │
│                   │                   │                     │
│                   │  [流式输出中...]    │                     │
└───────────────────┴───────────────────┴─────────────────────┘
```

### 会话生命周期

```
┌──────────┐    session/new     ┌──────────┐    session/prompt    ┌──────────┐
│  空闲状态  │ ────────────────► │  已创建    │ ──────────────────► │  执行中    │
│          │                    │          │                      │          │
└──────────┘                    └──────────┘                      └─────┬────┘
                                                                       │
                                            ┌──────────────────────────┘
                                            │ session/cancel
                                            ▼
                                      ┌──────────┐
                                      │  已取消    │
                                      │ (end_turn) │
                                      └──────────┘
```

## 声明式智能体加载

除了内置的三个智能体，服务端支持通过 JSON/YAML/TOML 文件声明式加载智能体。

### 声明文件格式

将声明文件放入 `agents_dir` 目录，服务启动时自动加载：

```json
// agents/coding.json
{
  "version": "1.0",
  "id": "coding-decl",
  "description": "代码专家智能体（声明式）",
  "instructions": "你是资深软件工程师。用中文回复，代码块使用 markdown 格式。",
  "model": {
    "provider": "deepseek",
    "model": "deepseek-v4-flash",
    "api_key": "$DEEPSEEK_API_KEY"
  },
  "tools": [
    { "type": "builtin", "name": "read_file" },
    { "type": "builtin", "name": "write_file" },
    { "type": "builtin", "name": "edit_file" }
  ],
  "max_tool_rounds": 15
}
```

### 支持的格式

| 扩展名 | 格式 | 需要 feature |
|--------|------|-------------|
| `.json` | JSON | 默认启用 |
| `.yaml` / `.yml` | YAML | `yaml` feature |
| `.toml` | TOML | `toml` feature |

### 智能体声明字段

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | string | 是 | 智能体唯一标识 |
| `description` | string | 否 | 功能描述 |
| `instructions` | string | 否 | 系统指令 |
| `model` | ModelConfig | 是 | 模型配置 |
| `tools` | ToolRef[] | 否 | 工具列表 |
| `context_providers` | ContextProviderDecl[] | 否 | 上下文提供器 |
| `max_tool_rounds` | usize | 否 | 最大工具调用轮数（默认 10） |
| `compression` | CompressionDecl | 否 | 压缩策略 |
| `sub_agents` | AgentDecl[] | 否 | 子代理声明（递归） |

### 工具引用类型

```json
// 内置工具
{ "type": "builtin", "name": "read_file" }

// Rhai 脚本工具
{
  "type": "rhai",
  "name": "my_custom_tool",
  "description": "自定义工具",
  "script_path": "./tools/my_tool.rhai",
  "parameters": { "type": "object", "properties": {} }
}

// 自定义工具（需注册工厂）
{ "type": "custom", "name": "my_tool", "config": {} }
```

## 目录结构

```
crates/host/
├── Cargo.toml                         # 包配置
├── agents/                             # 默认声明式智能体
│   ├── coding.json                     # 代码专家
│   ├── general.json                    # 通用助手
│   └── analysis.json                   # 数据分析师
└── src/
    ├── main.rs                         # 二进制入口
    ├── lib.rs                          # 库入口 + 公共 API
    ├── config.rs                       # 配置管理（figment + clap）
    ├── handler/
    │   ├── mod.rs
    │   ├── acp_agent.rs                # ACP Agent 处理器组装
    │   └── prompt.rs                   # session/prompt 核心桥接
    ├── registry/
    │   ├── mod.rs
    │   └── agent_registry.rs           # 多智能体注册中心
    ├── bridge/
    │   ├── mod.rs
    │   ├── types.rs                    # RAF → ACP 类型转换
    │   ├── session.rs                  # ACP ↔ RAF 会话桥接
    │   └── tracker.rs                  # 子代理状态追踪器
    ├── agents/
    │   ├── mod.rs
    │   ├── factory.rs                  # 内置智能体工厂
    │   └── loader.rs                   # 声明式加载器
    └── transport/
        ├── mod.rs
        ├── stdio.rs                    # Stdio 传输
        └── websocket.rs                # WebSocket (axum) 传输
```

## 设计决策

| 决策 | 理由 |
|------|------|
| 使用官方 `agent-client-protocol` 而非自建协议 | ACP 是开源标准，Rust SDK v0.14 已被 Zed 编辑器验证；自建协议会导致互操作性问题 |
| Stdio 作为主传输，WebSocket 作为备选 | 符合 ACP v1 标准——本地代理通过子进程 stdio 通信；WebSocket 用于远程部署 |
| 通过 `_meta` 标签承载子代理来源 | ACP 官方扩展机制，所有类型皆有 `_meta` 字段；客户端按 `agent_id` 分组即可实现多视图 |
| 子代理通过独立 session 直接调用 | ACP session 模型天然支持多会话并行；N 个 session 可同时运行 N 个子代理 |
| `_raf/subagent_list` 递归遍历 `get_subagent()` | 利用 RAF 原生子代理发现机制，支持任意深度的代理树 |
| `SubAgentStatusTracker` 发送状态变化信号 | 编排模式下子代理可能不产生文本内容，通过状态信号让客户端知道执行进度 |
| `AgentRegistry` 独立于 ACP 连接 | 多个客户端连接共享同一组智能体实例 |
| figment 分层配置 | TOML 文件 + 环境变量 + CLI 参数，生产部署友好 |
| 内置三个预设智能体 + 声明式加载 | 兼顾开箱即用和灵活扩展 |

## 相关链接

- [Agent Client Protocol 官方文档](https://agentclientprotocol.com/)
- [ACP Rust SDK (agent-client-protocol)](https://docs.rs/agent-client-protocol)
- [ACP Cookbook](https://docs.rs/agent-client-protocol-cookbook)
- [Rust Agent Framework](https://gitcode.com/rf2026/rust-agent-framework)

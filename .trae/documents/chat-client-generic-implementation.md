# Plan: 通用 ChatClient 实现 + OpenAI/DeepSeek 派生客户端（基于官方 API 标准）

## 1. Summary

在 `crates/client` 中实现通用的 `ChatClient` 基类，打通 LLM 流式对话通道。派生 `OpenAiChatClient` 和 `DeepSeekChatClient`，各自实现 provider 特有 API（模型列表、用量统计、缓存命中、thinking 模式等）。`ChatClient` 本身实现 `IChatClient`，派生客户端通过组合内嵌 `ChatClient` 实现委托。

## 2. 当前状态分析

- `OpenAIChatClient` 当前是 **stub/echo 实现**，无真实 HTTP 调用
- 无 `reqwest` 等 HTTP 依赖
- `IChatClient` trait 只有 `run()` 和 `model_id()` 两个方法
- `ChatClientConfig` 仅有基础字段（api_base、api_key、model、max_tokens、temperature）
- workspace 层已有 `futures-util`、`serde`、`serde_json`、`tokio`、`tracing`

## 3. 官方 API 标准分析

### 3.1 OpenAI 与 DeepSeek API 差异对比

| 维度 | OpenAI | DeepSeek |
|------|--------|----------|
| **Base URL** | `https://api.openai.com/v1` | `https://api.deepseek.com`（**无 /v1 前缀**） |
| **Chat 端点** | `POST /v1/chat/completions` | `POST /chat/completions` |
| **Models 端点** | `GET /v1/models` | `GET /models` |
| **Beta Base URL** | N/A | `https://api.deepseek.com/beta` |
| **Anthropic URL** | N/A | `https://api.deepseek.com/anthropic` |
| **Stream 选项** | `stream: true` | `stream: true` + `stream_options: {include_usage: true}` |
| **Thinking 模式** | N/A | `thinking: {type: "enabled"/"disabled"}` |
| **Reasoning Effort** | N/A | `reasoning_effort: "high"/"max"` |
| **Reasoning Content** | N/A | 响应含 `reasoning_content` 字段 |
| **Cache 命中** | N/A | `usage.prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` |
| **frequency_penalty** | 支持 | **已废弃**，传入无效果 |
| **presence_penalty** | 支持 | **已废弃**，传入无效果 |
| **user 字段** | `user` | `user_id` |
| **模型** | gpt-4, gpt-4o, gpt-3.5-turbo... | agnes-2.0-flash, deepseek-v4-pro |
| **Strict Tool Calls** | N/A | Beta 功能 |
| **FIM 补全** | N/A | Beta 功能 (`/completions`) |
| **Prefix Completion** | N/A | Beta 功能（最后一条 assistant 设 prefix: true） |

### 3.2 流式响应格式（双方一致）

```
data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","created":...,"model":"...","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","created":...,"model":"...","choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}

data: [DONE]
```

流式 chunk 中的 Delta 字段：
- `delta.content` → text_delta
- `delta.reasoning_content`（DeepSeek 特有） → 思维链 delta
- `delta.tool_calls[0].{index, id?, function.name?, function.arguments?}` → tool_call_delta

### 3.3 Usage 统计（DeepSeek 兼容 OpenAI 并有扩展）

```json
{
  "completion_tokens": 10,
  "prompt_tokens": 16,
  "total_tokens": 26,
  "prompt_cache_hit_tokens": 8,    // DeepSeek 扩展
  "prompt_cache_miss_tokens": 8,   // DeepSeek 扩展
  "completion_tokens_details": {
    "reasoning_tokens": 0          // DeepSeek 扩展
  }
}
```

## 4. 文件变更清单

### 4.1 新增文件

| 文件 | 用途 |
|------|------|
| `crates/client/src/chat_client.rs` | 通用 ChatClient 基类，IChatClient impl + HTTP SSE |
| `crates/client/src/transport.rs` | SSE 字节流 → ChatStreamChunk 解析（含 reasoning_content 支持） |
| `crates/client/src/deepseek_client.rs` | DeepSeekChatClient（组合 ChatClient + 特有 API） |
| `crates/client/src/types.rs` | Provider 专用类型 |

### 4.2 修改文件

| 文件 | 变更内容 |
|------|----------|
| `Cargo.toml` (workspace) | 添加 `reqwest` |
| `crates/client/Cargo.toml` | 添加 `reqwest` |
| `crates/client/src/config.rs` | extra_headers、extra_body、beta 开关、base_url 路径处理 |
| `crates/client/src/openai_client.rs` | 重构为组合 ChatClient |
| `crates/client/src/lib.rs` | 更新导出 |
| `crates/cli/src/main.rs` | 使用新客户端演示 |
| `crates/cli/Cargo.toml` | 添加 `rust-agent-macros` 依赖（修复 #[tool] 路径） |

## 5. 详细设计

### 5.1 依赖层

workspace `Cargo.toml` 新增：
```toml
reqwest = { version = "0.12", default-features = false, features = ["stream", "json", "rustls-tls"] }
```

### 5.2 `ChatClientConfig` 设计（增强版）

```rust
pub struct ChatClientConfig {
    pub api_base: String,       // "https://api.openai.com/v1" or "https://api.deepseek.com"
    pub api_key: String,        // #[serde(skip)]
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    pub extra_headers: HashMap<String, String>,
    pub extra_body: HashMap<String, serde_json::Value>,
    pub timeout_secs: Option<u64>,
}

impl ChatClientConfig {
    pub fn openai(model: &str, api_key: &str) -> Self {
        Self {
            api_base: "https://api.openai.com/v1".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }

    pub fn deepseek(model: &str, api_key: &str) -> Self {
        Self {
            api_base: "https://api.deepseek.com".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }
}
```

**关键设计决策**：不再硬编码 `/v1` 路径，改为 `api_base` 存储完整基础 URL（如 `https://api.deepseek.com`），`ChatClient` 内部拼接时使用 `{api_base}/chat/completions`。这样同时兼容 OpenAI（`/v1/chat/completions`）和 DeepSeek（`/chat/completions`）。

### 5.3 `types.rs` — Provider 类型

```rust
/// 模型列表条目（GET /v1/models 或 /models 响应中的 data[] 元素）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelListEntry {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

/// 用法统计（从 stream_options: {include_usage: true} 最后一个 chunk）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsageStats {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// DeepSeek 特有：缓存命中 token 数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_hit_tokens: Option<u32>,
    /// DeepSeek 特有：缓存未命中 token 数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_miss_tokens: Option<u32>,
    /// DeepSeek 特有：推理 token 数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
}

/// 缓存命中信息
#[derive(Debug, Clone, Default)]
pub struct CacheHitInfo {
    pub cache_hit_tokens: u32,
    pub cache_miss_tokens: u32,
    pub cache_hit_ratio: f64,
}

/// 推理/思考强度
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
}

impl ThinkingConfig {
    pub fn enabled() -> Self { Self { thinking_type: "enabled".into() } }
    pub fn disabled() -> Self { Self { thinking_type: "disabled".into() } }
}
```

### 5.4 `transport.rs` — SSE 流解析器

```rust
/// 将 reqwest 字节流解析为 ChatStreamChunk 流
pub fn parse_sse_stream(
    byte_stream: impl Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
) -> impl Stream<Item = Result<ChatStreamChunk, AgentError>> + Send
```

解析逻辑（基于官方 SSE 格式）：
1. 维护字节缓冲区
2. 按 `\n` 分割行
3. 匹配 `data: ` 前缀
4. `data: [DONE]` → 流结束
5. JSON 反序列化 → 提取 `choices[0].delta.{content, reasoning_content?, tool_calls?}`
6. 映射到 `ChatStreamChunk`

**重要**：`reasoning_content` 是 DeepSeek 特有字段，需要在 `ChatStreamChunk` 中新增字段支持：

```rust
// core/src/message.rs 新增字段
pub struct ChatStreamChunk {
    pub text_delta: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub reasoning_delta: Option<String>,   // DeepSeek 思维链 delta
}
```

同样 `AgentStreamChunk` 也需要对应新增：
```rust
pub struct AgentStreamChunk {
    pub text_delta: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub reasoning_delta: Option<String>,
    pub source_agent_id: Option<AgentId>,
}
```

### 5.5 `chat_client.rs` — 通用基类

```rust
pub struct ChatClient {
    http: reqwest::Client,
    config: ChatClientConfig,
}

impl ChatClient {
    pub fn new(config: ChatClientConfig) -> Result<Self>
    pub fn config(&self) -> &ChatClientConfig

    /// POST /chat/completions → SSE 流解析
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<BoxStream<'static, Result<ChatStreamChunk>>>

    /// 构造请求体（兼容 OpenAI + DeepSeek）
    fn build_request_body(&self, messages: &[ChatMessage]) -> serde_json::Value {
        // { model, messages, stream: true, max_tokens?, temperature?, top_p?,
        //   stop?, stream_options: { include_usage: true }, ...extra_body }
    }
}

#[async_trait]
impl IChatClient for ChatClient {
    async fn run(&self, messages: &[ChatMessage]) -> Result<BoxStream<Result<ChatStreamChunk>>>>
    fn model_id(&self) -> &str
}
```

请求体构建逻辑（兼容双方）：
- 始终设置 `stream: true`
- 始终设置 `stream_options: { include_usage: true }`（OpenAI + DeepSeek 均支持）
- `extra_body` 合并到请求体顶层（用于 DeepSeek 的 `thinking` 等字段）
- 使用 `extra_headers`（如 `OpenAI-Organization`）
- `Authorization: Bearer {api_key}` 标准认证头
- 不再拼接 `/v1` 前缀，直接使用 `{api_base}/chat/completions`

### 5.6 `openai_client.rs` — 重构

```rust
pub struct OpenAiChatClient {
    inner: ChatClient,
}

impl OpenAiChatClient {
    pub fn new(config: ChatClientConfig) -> Result<Self>

    /// GET /v1/models → Vec<ModelListEntry>
    pub async fn list_models(&self) -> Result<Vec<ModelListEntry>>

    /// GET /v1/usage → UsageStats
    pub async fn get_usage(&self) -> Result<UsageStats>
}

#[async_trait]
impl IChatClient for OpenAiChatClient { /* 委托 inner */ }
```

`list_models` 实现：
- GET `{api_base}/models`
- 解析 `{ "object": "list", "data": [...] }` → `Vec<ModelListEntry>`

### 5.7 `deepseek_client.rs` — 新增

DeepSeek 官方 Base URL = `https://api.deepseek.com`（**无 /v1**），Beta = `https://api.deepseek.com/beta`。

```rust
pub struct DeepSeekChatClient {
    inner: ChatClient,
}

impl DeepSeekChatClient {
    pub fn new(config: ChatClientConfig) -> Result<Self>

    /// 开启/关闭 thinking 模式
    /// 官方: extra_body={"thinking": {"type": "enabled/disabled"}}
    pub fn enable_thinking(&mut self, enabled: bool)

    /// 设置推理强度
    /// 官方: reasoning_effort="high"/"max"
    pub fn set_reasoning_effort(&mut self, effort: ReasoningEffort)

    /// GET /models → Vec<ModelListEntry>
    pub async fn list_models(&self) -> Result<Vec<ModelListEntry>>

    /// 从使用统计中提取缓存命中信息
    pub async fn get_cache_info(&self) -> Result<CacheHitInfo>
}

#[async_trait]
impl IChatClient for DeepSeekChatClient { /* 委托 inner */ }
```

### 5.8 `core/src/message.rs` — reasoning_delta 字段追加

```rust
pub struct ChatStreamChunk {
    pub text_delta: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub reasoning_delta: Option<String>,   // DeepSeek thinking mode
}

pub struct AgentStreamChunk {
    pub text_delta: Option<String>,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub reasoning_delta: Option<String>,
    pub source_agent_id: Option<AgentId>,
}
```

### 5.9 `crates/client/src/lib.rs` — 更新导出

```rust
pub mod chat_client;
pub mod config;
pub mod deepseek_client;
pub mod openai_client;
pub mod transport;
pub mod types;

pub use chat_client::ChatClient;
pub use config::ChatClientConfig;
pub use deepseek_client::DeepSeekChatClient;
pub use openai_client::OpenAiChatClient;
pub use types::*;
```

### 5.10 `crates/cli/src/main.rs` — 更新

```rust
// 演示 OpenAI 客户端
let oai_config = ChatClientConfig::openai("gpt-4.1-mini", "<key>");
let oai_client = OpenAiChatClient::new(oai_config)?;
let models = oai_client.list_models().await?;
println!("Available models: {:?}", models.iter().map(|m| &m.id).collect::<Vec<_>>());

// 演示 DeepSeek 客户端 + thinking 模式
let ds_config = ChatClientConfig::deepseek("deepseek-v4-pro", "<key>");
let mut ds_client = DeepSeekChatClient::new(ds_config)?;
ds_client.enable_thinking(true);
ds_client.set_reasoning_effort(ReasoningEffort::High);

// 完整对话流程
let agent = ChatClientAgent::new("assistant", Arc::new(ds_client))
    .with_instructions("You are a helpful AI assistant.")
    .with_tools(tools);
// ...
```

## 6. 架构图

```mermaid
flowchart TB
    subgraph Client["crates/client"]
        direction TB
        CC["ChatClient\n(HTTP + SSE 流式)"]
        T["transport.rs\n(SSE data: 行解析)"]
        CFG["ChatClientConfig\n(api_base 无硬编码 /v1)"]
        TYPES["types.rs\n(ModelListEntry, UsageStats, ...)"]

        subgraph Providers["Provider 派生"]
            OA["OpenAiChatClient\n· list_models()\n· get_usage()"]
            DS["DeepSeekChatClient\n· list_models()\n· enable_thinking()\n· set_reasoning_effort()\n· get_cache_info()"]
        end
    end

    subgraph Core["crates/core"]
        ICT["IChatClient trait"]
        MSG["ChatStreamChunk\n(+reasoning_delta)"]
    end

    ICT -.->|impl| CC
    OA -->|组合| CC
    DS -->|组合| CC
    CC --> T
    CC --> CFG
    OA --> TYPES
    DS --> TYPES
    T --> MSG

    OA -->|HTTP "POST /v1/chat/completions"| OA_API["api.openai.com/v1"]
    DS -->|HTTP "POST /chat/completions"| DS_API["api.deepseek.com"]

    style CC fill:#bbdefb,color:#0d47a1
    style OA fill:#c8e6c9,color:#1a5e20
    style DS fill:#c8e6c9,color:#1a5e20
    style T fill:#fff3e0,color:#e65100
```

## 7. 假设与决策

1. **DeepSeek Base URL 无 /v1** — `api_base` 不硬编码路径，拼接时直接用 `{api_base}/chat/completions`、`{api_base}/models`
2. **OpenAI 与 DeepSeek 共用 SSE 解析器** — 除 `reasoning_content` 字段外，Delta 格式完全一致
3. **`reasoning_delta` 加入 `ChatStreamChunk`** — 作为可选字段，不影响 OpenAI 客户端
4. **组合优于继承** — provider 客户端通过组合 `ChatClient` 实现委托
5. **`reqwest` 0.12 + rustls-tls** — 无 openssl 依赖
6. **不修改 core 层接口** — `IChatClient` trait 保持不变，仅在 `ChatStreamChunk`/`AgentStreamChunk` 新增 optional 字段
7. **`extra_body` 机制** — provider 特有请求体字段（如 `thinking`）通过 `extra_body` 注入，不污染 `ChatClient` 核心逻辑
8. **不实现 Beta 功能** — FIM、Chat Prefix Completion、Strict Tool Calls 为 DeepSeek Beta 功能，本次不实现但预留扩展点

## 8. 实现步骤

| # | 步骤 | 文件 |
|---|------|------|
| 1 | workspace Cargo.toml 添加 `reqwest` 依赖 | `Cargo.toml` |
| 2 | client Cargo.toml 添加 `reqwest` | `crates/client/Cargo.toml` |
| 3 | core `ChatStreamChunk`/`AgentStreamChunk` 新增 `reasoning_delta` 字段 | `crates/core/src/message.rs` |
| 4 | 新增 types.rs — ModelListEntry、UsageStats、CacheHitInfo、ThinkingConfig、ReasoningEffort | `crates/client/src/types.rs` |
| 5 | 增强 config.rs — extra_headers、extra_body、top_p、stop、openai()/deepseek() 构造器 | `crates/client/src/config.rs` |
| 6 | 新增 transport.rs — SSE data: 行解析 + Delta JSON → ChatStreamChunk | `crates/client/src/transport.rs` |
| 7 | 新增 chat_client.rs — 通用 ChatClient + IChatClient impl | `crates/client/src/chat_client.rs` |
| 8 | 重构 openai_client.rs — OpenAiChatClient 组合 ChatClient + list_models/get_usage | `crates/client/src/openai_client.rs` |
| 9 | 新增 deepseek_client.rs — DeepSeekChatClient + enable_thinking/set_reasoning_effort/list_models/get_cache_info | `crates/client/src/deepseek_client.rs` |
| 10 | 更新 lib.rs — 导出所有新类型 | `crates/client/src/lib.rs` |
| 11 | 更新 cli — 使用新客户端 + 对话流程 | `crates/cli/src/main.rs` |
| 12 | cargo check 全量编译验证 | 全 workspace |

## 9. 验证

```bash
cargo check --workspace
```

验证要点：
- `ChatClient` 实现 `IChatClient`，可放入 `Arc<dyn IChatClient>`
- `OpenAiChatClient` / `DeepSeekChatClient` 均可通过 `Arc<dyn IChatClient>` 使用
- `ReasoningEffort` 枚举值匹配官方（`High`、`Max`）
- `ThinkingConfig` 序列化为 `{"type": "enabled/disabled"}`
- `list_models()` 返回 `ModelListEntry` 解析后的结果
- `StreamingChunk` 新增的 `reasoning_delta` 保持向后兼容（`Option<String>`）
- CLI 可通过 `cargo run -p rust-agent-cli` 启动
- 模块化分离：transport / types / chat_client / openai_client / deepseek_client 各一个文件

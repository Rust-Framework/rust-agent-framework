# rust-agent-framework

Agent 运行时与工具层 —— 将 `IChatClient`、`ITool`、`IContextProvider` 组装为可执行 Agent 的核心引擎。是 `rust-agent-framework` 工作空间中最主要的 crate，提供完整的 Agent 生命周期管理、13 个内置工具、上下文提供器链、技能系统、压缩策略、记忆系统和流式输出转换。

## 目录

- [快速上手](#快速上手)
- [AgentBuilder —— 构建 Agent](#agentbuilder--构建-agent)
- [自定义工具](#自定义工具)
- [人机协作工具审批](#人机协作工具审批)
- [上下文提供器（ContextProvider）](#上下文提供器contextprovider)
- [会话管理（Session）](#会话管理session)
- [Agent 技能（Skills）](#agent-技能skills)
- [上下文压缩](#上下文压缩)
- [记忆系统（Memory）](#记忆系统memory)
- [多 Agent 路由](#多-agent-路由)
- [流式输出处理](#流式输出处理)
- [内置工具完整说明](#内置工具完整说明)
- [最佳实践](#最佳实践)
- [常见问题](#常见问题)
- [依赖关系](#依赖关系)

---

## 快速上手

### 最小可运行示例

```rust
use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_core::{AgentSession, ChatMessage, Content, ISession};
use rust_agent_framework::{tool, AgentBuilder};
use std::sync::Arc;

// 1. 用 #[tool] 宏定义一个工具
#[tool(description = "将输入文本原样返回")]
async fn echo(#[param(desc = "要回显的文本")] text: String) -> String {
    text
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 2. 创建 LLM 客户端
    let client = DeepSeekChatClient::new(
        ChatClientOptions::deepseek("deepseek-chat", "your-api-key")
    )?;

    // 3. 用 AgentBuilder 构建 Agent
    let agent = AgentBuilder::new("my-assistant")
        .chat_client(client)
        .instructions("你是一个有用的助手。当要求重复时使用 echo 工具。")
        .with_tool(Echo)       // 注册工具
        .build()?;

    // 4. 创建会话（保存对话历史）
    let session: Arc<dyn ISession> = Arc::new(AgentSession::new());

    // 5. 调用 run() 获取流式输出
    let messages = vec![ChatMessage::user("你好！请回显：hello world")];
    let mut stream = agent.run(messages, Some(session), None).await?;

    // 6. 逐块消费流
    use futures_util::StreamExt;
    while let Some(Ok(chunk)) = stream.next().await {
        for content in &chunk.contents {
            match content {
                Content::Text(t) => print!("{}", t.delta),
                Content::ToolCalling(tc) => println!("\n[调用工具: {}]", tc.name),
                Content::ToolCalled(tc) => println!("[工具结果: {}]", tc.result.as_deref().unwrap_or("错误")),
                _ => {}
            }
        }
        if let Some(fr) = &chunk.finish_reason {
            println!("\n[完成: {:?}]", fr);
        }
    }
    Ok(())
}
```

### 使用 OpenAI

只需更换客户端，其余代码完全相同：

```rust
use rust_agent_client::{ChatClientOptions, OpenAIChatClient};

let client = OpenAIChatClient::new(
    ChatClientOptions::openai("gpt-4o", "your-api-key")
)?;
// AgentBuilder 的用法完全一致 —— 框架对模型提供商透明
```

---

## AgentBuilder —— 构建 Agent

`AgentBuilder` 是构建 Agent 的推荐入口。它自动组装 ChatClient 管道、注入默认配置、并在有工具时包装 `FunctionInvokingChatClient`。

### 完整构建示例

```rust
use rust_agent_framework::AgentBuilder;
use rust_agent_core::ApprovalRequiredTool;
use std::sync::Arc;

let agent = AgentBuilder::new("production-assistant")
    // ── 必需：LLM 客户端 ──
    .chat_client(client)

    // ── 系统指令 ──
    .instructions("你是一个生产环境运维助手。操作前必须获得审批。")

    // ── 工具注册 ──
    .with_tool(ReadFile)                                      // 自动执行
    .with_tool(WriteFile)                                     // 自动执行
    .with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand))) // 需要审批

    // ── 工具循环最大轮次（默认 10） ──
    .max_tool_rounds(5)

    // ── 上下文提供器 ──
    .add_context_provider(MyRagProvider::new("knowledge-base/"))
    .add_context_provider(MyAuditProvider::new())

    // ── 压缩策略 ──
    .with_compression_strategy(Arc::new(SlidingWindowStrategy::new(4096)))
    .with_token_counter(Arc::new(SimpleTokenCounter))

    // ── 描述 ──
    .with_description("生产环境运维助手，所有写操作需审批")

    // ── 构建 ──
    .build()?;  // 返回 Arc<dyn IAgent>
```

### AgentBuilder 的默认行为

| 默认项 | 值 | 说明 |
|---|---|---|
| `InMemoryHistoryProvider` | 自动注入为首个 ContextProvider | 自动从 Session 注入历史消息 |
| `max_tool_rounds` | 10 | 工具调用循环的最大次数 |
| `tools` | 空 | 无工具时不会包装 `FunctionInvokingChatClient` |

### Build 流程（内部机制）

```mermaid
flowchart TD
    Input["用户调用: AgentBuilder::new(...) ... .build()"] --> Check{"有工具注册?"}
    Check -->|是| Wrap["ChatClientBuilder 包装:\nleaf(IChatClient)\n  → FunctionInvokingChatClient"]
    Check -->|否| Direct["直接使用 leaf IChatClient"]
    Wrap --> Create["创建 ChatClientAgent:\n· instructions\n· tools → ToolRegistry\n· context_providers"]
    Direct --> Create
    Create --> Output["返回 Arc<IAgent>"]
```

---

## 自定义工具

### 方式一：`#[tool]` 过程宏（推荐）

```rust
use rust_agent_framework::tool;

/// 获取城市天气
#[tool(description = "获取指定城市的当前天气信息")]
async fn get_weather(
    #[param(desc = "城市名称，如 '北京'、'Shanghai'")] city: String,
    #[param(desc = "温度单位：celsius 或 fahrenheit")] unit: Option<String>,
) -> String {
    let unit = unit.unwrap_or_else(|| "celsius".into());
    // 实际项目中应调用天气 API
    format!("{} 当前温度：22°{}", city, unit)
}
```

过程宏自动生成：
- 一个名为 `GetWeather`（PascalCase）的 struct
- `ITool` trait 的完整实现（`name`、`description`、`parameters`、`execute`）
- 自动从 Rust 类型推导 JSON Schema

### Rust 类型到 JSON Schema 的映射

| Rust 类型 | 生成的 JSON Schema |
|---|---|
| `String`, `&str` | `{"type": "string"}` |
| `i64`, `u32` 等整数 | `{"type": "integer"}` |
| `f64` 等浮点 | `{"type": "number"}` |
| `bool` | `{"type": "boolean"}` |
| `Option<T>` | 同 `T`，标记为非必填 |
| `Vec<T>` | `{"type": "array", "items": {...}}` |

### 方式二：手动实现 `ITool` trait

当工具逻辑需要外部状态（数据库连接、配置等）时，手动实现：

```rust
use async_trait::async_trait;
use rust_agent_core::{ITool, Result};
use serde_json::json;

struct DatabaseTool {
    pool: sqlx::PgPool,
}

#[async_trait]
impl ITool for DatabaseTool {
    fn name(&self) -> &str {
        "db_query"
    }

    fn description(&self) -> &str {
        "执行参数化 SQL 查询并返回结果"
    }

    fn kind(&self) -> &str {
        "function"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "sql": {
                    "type": "string",
                    "description": "SQL 查询语句"
                }
            },
            "required": ["sql"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        let sql = arguments["sql"].as_str().unwrap_or("");
        let rows = sqlx::query(sql).fetch_all(&self.pool).await
            .map_err(|e| rust_agent_core::AgentError::ToolError(e.to_string()))?;
        Ok(serde_json::to_string(&rows).unwrap_or_default())
    }
}
```

### 方式三：`#[tool]` 宏 + 静态全局配置

当工具需要外部状态（数据库连接池、API 密钥、配置文件等），又不想放弃 `#[tool]` 宏的便利时，可以使用 `std::sync::OnceLock` 定义全局静态对象来传递配置。**关键是在 Agent 启动前完成配置初始化。**

```rust
use rust_agent_framework::tool;
use std::sync::OnceLock;
use sqlx::PgPool;

// 1. 定义全局静态配置（OnceLock 保证只初始化一次）
static DB_POOL: OnceLock<PgPool> = OnceLock::new();

// 2. 初始化函数 —— 在 Agent 构建前调用
pub fn init_db_pool(database_url: &str) {
    let pool = PgPool::connect_lazy(database_url)
        .expect("数据库连接失败");
    DB_POOL.set(pool).expect("DB_POOL 已初始化");
}

// 3. #[tool] 宏中直接访问静态配置
#[tool(description = "执行参数化 SQL 查询并返回结果")]
async fn db_query(
    #[param(desc = "SQL 查询语句")] sql: String,
) -> String {
    let pool = DB_POOL.get()
        .expect("DB_POOL 未初始化，请先调用 init_db_pool()");
    let rows = sqlx::query(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("查询失败: {}", e))?;
    serde_json::to_string(&rows).unwrap_or_default()
}
```

完整使用示例：

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ⚠️ 必须在 AgentBuilder::build() 之前初始化
    init_db_pool("postgres://user:pass@localhost/mydb");

    // 正常构建 Agent，工具已经是零参数的 struct
    let agent = AgentBuilder::new("db-assistant")
        .chat_client(client)
        .with_tool(DbQuery)  // 不需要传入 pool！
        .build()?;

    // 使用 Agent ...
    Ok(())
}
```

**模式对比：**

| 维度 | 手动实现 `ITool` | `#[tool]` + `OnceLock` |
|---|---|---|
| 代码量 | 多（~40 行） | 少（~15 行 + 5 行初始化） |
| JSON Schema | 手写 | 自动推导 |
| 外部状态 | `self.pool` 直接访问 | 通过 `OnceLock::get()` 间接访问 |
| 适用场景 | 复杂工具、多实例状态隔离 | 全局单例（连接池、配置等） |

**使用静态配置的注意事项：**

- **`OnceLock::set()` 必须在 `build()` 之前调用** —— 框架构建完 Agent 后 LLM 随时可能调用工具。顺序错误会导致 `OnceLock::get()` 返回 `None` 而 panic
- **`OnceLock` 只能 set 一次** —— 适合不可变的全局配置（连接池、密钥等）。如需运行时切换配置，改用 `RwLock<Option<T>>`
- **测试中使用 `std::sync::Once` 配合** —— 避免多个测试并行初始化冲突
- **生产环境中建议用 `OnceLock` 而非 `lazy_static!`** —— `OnceLock` 是标准库，无需外部依赖

```rust
// 进阶：使用 RwLock 支持运行时切换配置
static CONFIG: std::sync::RwLock<Option<AppConfig>> = std::sync::RwLock::new(None);

pub fn update_config(config: AppConfig) {
    *CONFIG.write().unwrap() = Some(config);
}

#[tool(description = "根据当前配置执行操作")]
async fn config_aware_tool(#[param(desc = "操作名")] action: String) -> String {
    let config = CONFIG.read().unwrap();
    match config.as_ref() {
        Some(cfg) => format!("使用配置 {} 执行 {}", cfg.mode, action),
        None => "配置未初始化".into(),
    }
}
```

### 注册工具的方式

```rust
// 方式 A：AgentBuilder 单工具注册
AgentBuilder::new("agent")
    .with_tool(GetWeather)
    .with_tool(DatabaseTool { pool: pool.clone() })
    .build()?;

// 方式 B：ToolRegistry 批量注册
let mut registry = ToolRegistry::new();
registry.register(GetWeather);
registry.register(DatabaseTool { pool });
// 然后通过 ChatClientAgent::with_tools(registry) 注入

// 方式 C：一次性注册所有内置工具
use rust_agent_framework::tools::register_all;
let mut registry = ToolRegistry::new();
register_all(&mut registry);
```

---

## 人机协作工具审批

框架支持工具执行前的人工审批（Human-in-the-Loop），参考 MAF 的 `ApprovalRequiredAIFunction` 设计。

### 核心概念

```mermaid
flowchart TD
    subgraph 第一次调用 ["agent.run() 第一次调用"]
        LLM["LLM 返回 tool_calls"] --> Check{"工具 requires_approval()?"}
        Check -->|否| AutoExec[执行工具，结果返回 LLM 继续循环]
        Check -->|是| EmitReq["发出 ToolApprovalRequest 事件"]
        EmitReq --> Persist["将 assistant(tool_calls) 持久化到 Session"]
        Persist --> Pause["以 Finish(AwaitingApproval) 暂停流"]
    end

    subgraph 调用方 ["调用方（用户交互层）"]
        Pause --> Show[展示审批请求给用户]
        Show --> Decide[用户决定 批准/拒绝]
        Decide --> BuildResp[构建 Vec<ToolApprovalResponse>]
    end

    subgraph 第二次调用 ["agent.run() 第二次调用（恢复）"]
        BuildResp --> ResumeOpt["传入 options.tool_approval_responses"]
        ResumeOpt --> Detect["FunctionInvokingChatClient 检测到待审批响应"]
        Detect --> Resolve["执行已批准的工具 / 为拒绝的工具生成拒绝消息"]
        Resolve --> Continue["将工具结果添入消息，调用 LLM 继续对话"]
    end
```

### 标记工具需要审批

使用 `ApprovalRequiredTool` 包装器，在 **Agent 构建时** 决定审批策略：

```rust
use rust_agent_core::ApprovalRequiredTool;
use std::sync::Arc;

// 开发环境 Agent：所有工具自动执行
let dev_agent = AgentBuilder::new("dev-assistant")
    .chat_client(client.clone())
    .with_tool(RunCommand)    // 自动执行
    .with_tool(WriteFile)     // 自动执行
    .build()?;

// 生产环境 Agent：敏感工具需要审批
let prod_agent = AgentBuilder::new("prod-assistant")
    .chat_client(client.clone())
    .with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand)))  // 需要审批
    .with_tool(WriteFile)                                         // 自动执行
    .build()?;
```

设计要点：
- **审批标记在构建时决定，而非在工具定义上** —— 同一个 `RunCommand` 工具，在不同 Agent 中可以有不同策略
- **全有或全无** —— LLM 单次响应中只要有任意一个工具触发审批，该响应中所有工具都暂停等待
- **`ITool::requires_approval()` 默认返回 `false`**，只有 `ApprovalRequiredTool` 重写为 `true`

### 完整的审批循环代码

```rust
use rust_agent_core::{
    AgentResponseUpdate, FinishReason, ToolApprovalResponse, AgentRunOptions, Content,
};
use futures_util::StreamExt;
use std::sync::Arc;

async fn interactive_loop(
    agent: &Arc<dyn IAgent>,
    session: Arc<dyn ISession>,
) -> anyhow::Result<()> {
    let mut user_input = String::new();

    loop {
        println!("> ");
        std::io::stdin().read_line(&mut user_input)?;
        let messages = vec![ChatMessage::user(user_input.trim())];

        // 内层循环：处理多次 run()，直到不再需要审批
        let mut messages = messages;
        loop {
            let mut stream = agent.run(messages, Some(session.clone()), None).await?;

            let mut finish_reason = None;
            // 消费流，获取审批请求
            while let Some(Ok(chunk)) = stream.next().await {
                for content in &chunk.contents {
                    match content {
                        Content::Text(t) => print!("{}", t.delta),
                        _ => {}
                    }
                }
                finish_reason = chunk.finish_reason.clone();
            }

            match finish_reason {
                Some(FinishReason::AwaitingApproval) => {
                    // 需要审批 —— 收集用户决定
                    println!("\n--- 工具执行需要您的审批 ---");
                    // （实际项目中应展示审批请求详情）
                    let responses = collect_approvals().await?;

                    let resume_options = AgentRunOptions::new()
                        .with_tool_approval_responses(responses);
                    messages = vec![];  // Session 中已有上下文
                    // 继续内层循环，用审批响应恢复执行
                    continue;
                }
                _ => break,  // 对话正常结束
            }
        }
    }
}

async fn collect_approvals() -> anyhow::Result<Vec<ToolApprovalResponse>> {
    // 实际应用中应展示工具名称、参数供用户审核
    // 这里简化处理，默认批准
    Ok(vec![ToolApprovalResponse {
        call_id: "call_1".into(),
        approved: true,
        reason: None,
    }])
}
```

### 关键行为

| 场景 | 行为 |
|---|---|
| 工具被拒绝 | 拒绝原因会作为工具结果返回给 LLM（`"Rejected: 用户拒绝了此操作"`），LLM 可据此调整策略 |
| 部分批准部分拒绝 | 每个 `ToolApprovalResponse` 独立决定，批准的工具正常执行，拒绝的工具返回拒绝消息 |
| Session 恢复 | 审批暂停时，`assistant(tool_calls)` 消息已被持久化到 Session。恢复时无需传入任何 messages |
| 多次审批 | `run()` 恢复后 LLM 可能再次调用需要审批的工具，调用方应在循环中处理 |

---

## 上下文提供器（ContextProvider）

`IContextProvider` 是框架的核心扩展机制。每个 Provider 在 `run()` 调用前后执行，可注入指令、消息和动态工具。

### 生命周期

```
agent.run(messages, session, options)
    │
    ▼
┌─────────────────── on_invoking ───────────────────┐
│  Provider 1: InMemoryHistoryProvider               │
│    → 注入 Session 中的历史消息                     │
│                                                      │
│  Provider 2: SkillsProvider                        │
│    → 注入 Skills 的广告指令                         │
│    → 注入 load_skill / read_skill_resource 工具     │
│                                                      │
│  Provider 3: RAG Provider                          │
│    → 注入知识库检索结果                             │
│                                                      │
│  Provider 4: Compression Provider                   │
│    → 设置 replace_messages = true                   │
│    → 用压缩后消息替换累积消息                       │
└──────────────────────────────────────────────────────┘
    │
    ▼
LLM 调用 + 流式输出
    │
    ▼
┌─────────────────── on_invoked ────────────────────┐
│  各 Provider 收到 AgentResponse                    │
│    → InMemoryHistoryProvider 持久化新消息到 Session │
└──────────────────────────────────────────────────────┘
```

### 默认 Provider：InMemoryHistoryProvider

`AgentBuilder` 自动注入，负责：
- **on_invoking**：从 Session 读取所有历史消息，注入到当前调用的 messages 中
- **on_invoked**：原子批量持久化新增的消息，通过追踪消息数量避免重复

替换为自定义历史管理：

```rust
AgentBuilder::new("agent")
    .chat_client(client)
    .with_history_provider(MyRedisHistoryProvider::new(redis_client))
    .build()?;
```

### 编写自定义 Provider

```rust
use async_trait::async_trait;
use rust_agent_core::{
    ContextResult, IAgent, IContextProvider, ISession,
    ChatMessage, AgentRunOptions,
};

/// 将知识库中相关文档注入上下文
struct RagProvider {
    index_path: String,
}

#[async_trait]
impl IContextProvider for RagProvider {
    fn name(&self) -> &str {
        "RagProvider"
    }

    fn kind(&self) -> &str {
        "knowledge"
    }

    async fn on_invoking(
        &self,
        agent: &dyn IAgent,
        session: &dyn ISession,
        messages: &[ChatMessage],
        options: &AgentRunOptions,
    ) -> rust_agent_core::Result<ContextResult> {
        // 从最后一条用户消息中提取查询意图
        let query = messages.iter()
            .rev()
            .find(|m| m.role == rust_agent_core::MessageRole::User)
            .map(|m| &m.content)
            .unwrap_or(&String::new());

        // 检索相关文档（实现略）
        let docs = self.search(query)?;

        Ok(ContextResult {
            // 注入指令：告诉 LLM 使用提供的文档回答
            instructions: Some(
                "请仅基于以下参考文档回答问题。如果文档不包含相关信息，请如实告知。".into()
            ),
            // 注入消息：将检索到的文档插入上下文
            messages: vec![ChatMessage::user(format!(
                "参考文档：\n{}", docs
            ))],
            tools: vec![],
            // 不替换已有消息，追加在后面
            replace_messages: false,
        })
    }
}
```

### Provider 链的执行顺序

1. Provider 按注册顺序依次执行 `on_invoking()`
2. 靠前的 Provider 产生的消息会被靠后的 Provider 看到
3. 如果某个 Provider 设置 `replace_messages = true`，前面所有 Provider 产生的消息都会被清空替换
4. 经典链：`[HistoryProvider → RAG → Skills → Compression]`

---

## 会话管理（Session）

### 基本用法

Session 保存对话历史，跨多次 `run()` 调用持久化：

```rust
use rust_agent_core::{AgentSession, ISession};
use std::sync::Arc;

let session: Arc<dyn ISession> = Arc::new(AgentSession::new());

// 第一次调用
agent.run(
    vec![ChatMessage::user("我叫张三")],
    Some(session.clone()),
    None,
).await?;

// 第二次调用 —— messages 可以为空，历史从 Session 自动注入
agent.run(
    vec![ChatMessage::user("我叫什么名字？")],
    Some(session.clone()),
    None,
).await?;
// LLM 能回答"张三"，因为 Session 中有历史
```

### Session 特性

| 方法 | 说明 |
|---|---|
| `add_message(msg)` | 添加一条消息 |
| `get_messages()` | 获取所有消息 |
| `id()` | 获取 Session ID（UUID v4） |
| `metadata()` | 读写元数据 |
| `touch_request_hash(messages)` | KV Cache 追踪：计算请求哈希，判断是否命中缓存 |
| `serialize()` / `deserialize(data)` | 序列化/反序列化 |

### 持久化到文件系统

框架内置 `InMemorySessionStore` 和 `FileSystemSessionStore`：

```rust
use rust_agent_framework::InMemorySessionStore;

let store = InMemorySessionStore::new()
    .with_ttl(SessionTTLOptions { max_idle_secs: Some(3600), max_lifetime_secs: None, cleanup_interval_secs: 60 });

use rust_agent_framework::FileSystemSessionStore;
use std::path::PathBuf;

let store = FileSystemSessionStore::new(PathBuf::from("./sessions"));
store.save(session.id(), &session.serialize().await?).await?;
```

对于多租户场景，使用 `IsolationScopedSessionStore` 配合 `FixedIsolationKeyProvider`：

```rust
use rust_agent_framework::{IsolationScopedSessionStore, FixedIsolationKeyProvider};
use std::sync::Arc;

let inner = Arc::new(InMemorySessionStore::new());
let key_provider = Arc::new(FixedIsolationKeyProvider::new("tenant-1".into()));
let scoped_store = IsolationScopedSessionStore::new(inner, key_provider);
```

### 自定义持久化（实现 ISessionStore）

```rust
use async_trait::async_trait;
use rust_agent_core::ISessionStore;

struct PostgresSessionStore { pool: sqlx::PgPool }

#[async_trait]
impl ISessionStore for PostgresSessionStore {
    async fn save(&self, sid: &str, data: &str) -> rust_agent_core::Result<()> {
        sqlx::query("INSERT INTO sessions (id, data) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET data = $2")
            .bind(sid).bind(data).execute(&self.pool).await?;
        Ok(())
    }
    async fn load(&self, sid: &str) -> rust_agent_core::Result<Option<String>> {
        let row = sqlx::query_scalar("SELECT data FROM sessions WHERE id = $1")
            .bind(sid).fetch_optional(&self.pool).await?;
        Ok(row)
    }
    async fn delete(&self, sid: &str) -> rust_agent_core::Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = $1").bind(sid).execute(&self.pool).await?;
        Ok(())
    }
    async fn list(&self) -> rust_agent_core::Result<Vec<String>> {
        let ids = sqlx::query_scalar("SELECT id FROM sessions").fetch_all(&self.pool).await?;
        Ok(ids)
    }
}
```

---

## Agent 技能（Skills）

Skills 是基于 [Agent Skills 开放标准](https://agentskills.io/) 的可移植指令包，支持渐进式披露（Progressive Disclosure）—— Agent 只在需要时才加载完整上下文。

### 技能目录结构

```
code-review/
├── SKILL.md              # 必需：YAML frontmatter + Markdown 指令
├── scripts/              # 可选：可执行脚本
│   └── analyze.py
├── references/           # 可选：参照文档（按需加载）
│   ├── rust-guidelines.md
│   └── security-checklist.md
└── assets/               # 可选：模板和静态资源
    └── review-template.md
```

**SKILL.md 格式：**

```markdown
---
name: code-review
description: 审查代码变更，发现缺陷、回归和缺失测试。当用户要求代码审查时使用。
license: MIT
metadata:
  author: dev-team
  version: "1.0"
---

# 代码审查指南

## 审查流程
1. 理解变更意图
2. 检查逻辑正确性
3. 检查边界条件
4. 评估测试覆盖

详细检查清单在 `references/security-checklist.md`。
```

### 渐进式披露（Progressive Disclosure）

Skills 通过 4 个阶段最小化 Token 消耗：

| 阶段 | 触发条件 | 动作 | Token 消耗 |
|---|---|---|---|
| **广告** | 每次 Agent 调用 | 注入 `name` + `description` 到 system prompt | ~100/skill |
| **加载** | LLM 调用 `load_skill(name)` | 返回 SKILL.md 完整指令 | <5000 |
| **读取** | LLM 调用 `read_skill_resource(name, path)` | 返回 references/ 中的文档 | 按需 |
| **运行** | LLM 调用 `run_skill_script(name, path, args)` | 执行 scripts/ 中的脚本 | 按需 |

### 注册技能

**从目录扫描：**

```rust
use rust_agent_framework::AgentSkillsProvider;

// 扫描目录下所有 SKILL.md
let provider = AgentSkillsProvider::scan("./skills")?;

let agent = AgentBuilder::new("coder")
    .chat_client(client)
    .add_context_provider(provider)
    .build()?;
```

**手动注册：**

```rust
use rust_agent_framework::{AgentSkillsProvider, AgentSkill};

let provider = AgentSkillsProvider::new()
    .with_skill(AgentSkill::from_dir("./skills/code-review")?)
    .with_skill(AgentSkill::from_dir("./skills/git-ops")?);
```

**动态技能（从数据库/API）：**

```rust
let skill = AgentSkill::dynamic(
    SkillMetadata {
        name: "enterprise-policy".into(),
        description: "公司费用报销政策。".into(),
        ..Default::default()
    },
    || db.query_instructions("enterprise-policy"),  // 懒加载
);
```

### 脚本执行

`SubprocessScriptRunner` 自动按扩展名选择解释器：

| 扩展名 | 解释器 |
|---|---|
| `.py` | `python` |
| `.js` | `node` |
| `.sh` | `bash` |
| `.ps1` | `powershell -File` |

```rust
use rust_agent_framework::SubprocessScriptRunner;

let provider = AgentSkillsProvider::scan("./skills")?
    .with_script_runner(Arc::new(SubprocessScriptRunner));
```

### Skills 最佳实践

- **SKILL.md 控制在 500 行以内** —— 详细内容放到 `references/` 中，按需加载
- **description 包含使用关键词** —— 帮助 LLM 判断何时使用该技能。例如 `"当用户要求审查代码、检查代码质量或评估 Pull Request 时使用"`
- **技能命名用小写+连字符**：`code-review`、`git-ops`、`data-analysis`
- **一个技能一个领域** —— 每个技能覆盖单一任务，组合多个技能而非创建巨型技能
- **脚本自包含** —— 所有输入通过命令行参数传递，输出使用 JSON 格式（方便 LLM 解析）
- **References 中的大文件零 Token 消耗** —— 直到 LLM 显式调用 `read_skill_resource` 才加载

---

## 上下文压缩

当对话历史超过模型上下文窗口时，框架提供压缩策略来自动管理 Token 预算。

### 内置策略

| 策略 | 文件 | 说明 |
|---|---|---|
| `SlidingWindowStrategy` | `compression/sliding_window.rs` | 滑动窗口：保留最近 N 条消息，丢弃更早的 |
| `TokenBudgetStrategy` | `compression/token_budget.rs` | Token 预算：在预算范围内尽可能保留消息 |
| `CompressionPipeline` | `compression/pipeline.rs` | 管道组合：依次执行多个压缩策略 |

### 配置压缩

```rust
use rust_agent_framework::compression::SlidingWindowStrategy;
use rust_agent_framework::EstimateCounter;
use std::sync::Arc;

let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .with_compression_strategy(Arc::new(
        SlidingWindowStrategy::new(4096)  // 保留最近 4096 token 的消息
    ))
    .with_token_counter(Arc::new(EstimateCounter))
    .build()?;
```

压缩在 Phase 1.5 执行（Context Provider 之后、LLM 调用之前），仅在 token_counter + compression_strategy + model_metadata 三者都存在时生效。

---

## 记忆系统（Memory）

框架提供可选的记忆 Agent（`MemoryAgent`），在后台自动归纳和合并对话中的重要信息。

### 工作原理

```
用户消息 → Agent.run()
    │
    ├── 正常对话流程（LLM + 工具）
    │
    └── 后台异步
        └── MemoryAgent 定期运行
            ├── 读取 Session 中的新消息
            ├── 调用 LLM 归纳关键信息
            ├── 合并到已有的记忆存储中
            └── 通过 SkillMemoryContextProvider 在下一次调用时注入
```

### 启用记忆

```rust
use rust_agent_framework::memory::SkillMemoryContextProvider;

// SkillMemoryContextProvider 自动发现 Agent 的 ChatClient，
// 用其创建 MemoryAgent 进行后台记忆整合
let provider = SkillMemoryContextProvider::new("memory-store");

let agent = AgentBuilder::new("agent")
    .chat_client(client)
    .add_context_provider(provider)
    .build()?;
```

---

## 多 Agent 路由

注册多个 Agent 并按 ID 路由调用：

```rust
use std::collections::HashMap;
use std::sync::Arc;
use rust_agent_core::{AgentId, IAgent};

let mut agents: HashMap<AgentId, Arc<dyn IAgent>> = HashMap::new();

// 注册
agents.insert(coder_agent.id().clone(), coder_agent);
agents.insert(reviewer_agent.id().clone(), reviewer_agent);

// 查询
let all_ids: Vec<_> = agents.keys().collect();

// 按 ID 路由
if let Some(agent) = agents.get(&AgentId::new("coder")) {
    let stream = agent.run(messages, Some(session), None).await?;
}
```

对于需要子 Agent 树遍历、默认 Agent、Agent 元数据发现等高级功能的场景，`crates/host/src/registry/agent_registry.rs` 中的 `AgentRegistry` 提供了一个更完整的多 Agent 注册实现，适用于 ACP 协议宿主等生产环境。

---

## 流式输出处理

### AgentResponseResult 结构

每次 `run()` 返回 `BoxStream<AgentResponseResult>`，每个 chunk 包含：

```rust
pub struct AgentResponseResult {
    pub contents: Vec<Content>,      // 内容项
    pub events: Vec<Event>,          // 生命周期事件
    pub finish_reason: Option<FinishReason>, // 最后一个 chunk 非 None
    // ...
}
```

### Content 变体

| 变体 | 说明 | 使用场景 |
|---|---|---|
| `Content::Text(t)` | 文本增量 `t.delta` | 实时打字效果 |
| `Content::Reasoning(r)` | 思维链内容（DeepSeek R1）| 展示推理过程 |
| `Content::ToolCallStart(t)` | 工具调用开始 | 展示"正在调用 XX 工具" |
| `Content::ToolCallArgs(t)` | 参数流片段 | 实时展示参数构建 |
| `Content::ToolCallArgsParsed(t)` | 参数键值对已完整 | 逐字段展示（非整个 JSON） |
| `Content::ToolCallArgsProgress(t)` | 长字符串参数仍在中 | 进度条 |
| `Content::ToolCallEnd(t)` | 参数接收完毕 | 标记准备执行 |
| `Content::ToolCalling(t)` | 完整参数已解析 | 展示完整调用信息 |
| `Content::ToolCalled(t)` | 执行结果（含 `result`/`error`）| 展示结果 |
| `Content::Uri(t)` | URI 链接 | 点击跳转 |
| `Content::Error(t)` | 流处理错误 | 错误提示 |

### 完整流消费模板

```rust
use futures_util::StreamExt;
use rust_agent_core::Content;

let mut stream = agent.run(messages, Some(session), None).await?;
while let Some(Ok(chunk)) = stream.next().await {
    for content in &chunk.contents {
        match content {
            Content::Text(t) => print!("{}", t.delta),
            Content::Reasoning(r) => print!("[思考: {}]", r.delta),
            Content::ToolCallStart(tc) => {
                println!("\n🔧 准备调用: {}", tc.name);
            }
            Content::ToolCallArgs(ta) => {
                print!("{}", ta.args_delta);
            }
            Content::ToolCalled(tc) => {
                match (&tc.result, &tc.error) {
                    (Some(r), _) => println!("✅ 结果: {:.200}...", r),
                    (_, Some(e)) => println!("❌ 错误: {}", e),
                    _ => {}
                }
            }
            Content::Error(e) => eprintln!("❌ 流错误: {}", e.error_code),
            _ => {}
        }
    }

    // 检查是否结束
    match chunk.finish_reason {
        Some(rust_agent_core::FinishReason::Stop) => println!("\n[完成]"),
        Some(rust_agent_core::FinishReason::AwaitingApproval) => {
            println!("\n[需要审批 — 等待用户决策]");
        }
        Some(other) => println!("\n[结束: {:?}]", other),
        None => {}
    }
}
```

---

## 内置工具完整说明

所有 11 个内置工具位于 `src/tools/`，均以 `#[tool]` 宏定义。全部返回统一 JSON 格式 `{"ok": bool, "data": ..., "error": ...}`。

### 文件操作

| 工具 | 说明 | 限制 |
|---|---|---|
| `read_file` | 读取文件内容，支持行范围和 offset/limit | 最大 512KB |
| `write_file` | 创建或覆写文件，自动创建父目录 | — |
| `edit_file` | 精确字符串替换（old_string → new_string）| — |
| `list_files` | 列出目录内容 | — |
| `inspect_file` | 检查文件元数据（类型、大小、权限、行数）| — |
| `make_directory` | 递归创建目录 | — |
| `remove_path` | 删除文件或目录 | — |
| `move_file` | 移动/重命名文件 | — |
| `find_files` | 按 glob 模式搜索文件 | — |
| `search_file` | 按正则表达式搜索文件内容 | — |

### Shell 命令

| 工具 | 说明 | 限制 |
|---|---|---|
| `run_command` | 执行 shell 命令 | 输出上限 100KB，推荐使用 `ApprovalRequiredTool` 包装 |

### 一次性注册全部工具

```rust
use rust_agent_framework::tools::register_all;
use rust_agent_core::ToolRegistry;

let mut registry = ToolRegistry::new();
register_all(&mut registry);
```

---

## 最佳实践

### 1. 工具设计

**用 `#[tool]` 宏优先** —— 大部分场景下过程宏比手动实现更简洁且不易出错。仅在需要外部状态（数据库连接等）时才手动实现 `ITool`。

**为每个参数写描述** —— LLM 依赖 `#[param(desc = "...")]` 来理解如何传参：

```rust
// ✅ 好：有描述
#[tool(description = "发送邮件")]
async fn send_email(
    #[param(desc = "收件人邮箱")] to: String,
    #[param(desc = "邮件主题")] subject: String,
    #[param(desc = "邮件正文")] body: String,
) -> String { /* ... */ }

// ❌ 差：无描述，LLM 难以正确使用
#[tool(description = "发送邮件")]
async fn send_email(to: String, subject: String, body: String) -> String { /* ... */ }
```

### 2. 工具审批策略

**在 Agent 构建时决定审批策略，而非在工具定义上**：

```rust
// ✅ 好：不同 Agent 不同策略
AgentBuilder::new("dev").with_tool(RunCommand).build()?;
AgentBuilder::new("prod").with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand))).build()?;

// ❌ 差：在工具定义上硬编码审批要求
```

**生产环境中，所有写操作建议包装 `ApprovalRequiredTool`**：
- `run_command` —— 必然需要审批
- `write_file` —— 建议审批
- `remove_path` —— 必须审批
- `read_file` —— 通常不需要审批

### 3. Session 生命周期

**一个用户会话对应一个 Session** —— Session 在用户首次交互时创建，在整个对话过程中复用：

```rust
// ✅ 好：持久化 Session
let session: Arc<dyn ISession> = /* 从数据库加载或新建 */;
agent.run(messages1, Some(session.clone()), None).await?;
agent.run(messages2, Some(session.clone()), None).await?;
agent.run(messages3, Some(session.clone()), None).await?;

// ❌ 差：每次 run() 新建 Session —— 丢失历史
agent.run(messages1, Some(Arc::new(AgentSession::new())), None).await?;
agent.run(messages2, Some(Arc::new(AgentSession::new())), None).await?;
```

### 4. 流式输出处理

**在循环中消费流，而非 collect()** —— 实时输出提供更好的用户体验：

```rust
// ✅ 好：逐块消费，实时展示
while let Some(Ok(chunk)) = stream.next().await {
    for content in &chunk.contents {
        if let Content::Text(t) = content {
            print!("{}", t.delta);  // 打字效果
        }
    }
}

// ❌ 差：阻塞等待全部输出
let results: Vec<_> = stream.collect().await;  // 失去实时性
```

**检查 `FinishReason` 以区分结束原因** —— 特别是 `AwaitingApproval` 需要特殊处理。

### 5. 上下文管理

**合理安排 Provider 链顺序**：

```
[HistoryProvider → RAG → Skills → Compression]
    ↑               ↑       ↑         ↑
  注入历史       注入知识   注入技能   压缩总量
  (最早)                               (最晚)
```

**开启压缩时务必设置 token_counter** —— 否则压缩策略不会生效。

**Skills 使用渐进式披露** —— 不要把大量文档直接放入 `SKILL.md`，放到 `references/` 中按需加载。

### 6. 错误处理

```rust
// ✅ 好：安全消费流，区分错误类型
while let Some(item) = stream.next().await {
    match item {
        Ok(chunk) => { /* 正常处理 */ }
        Err(e) => {
            tracing::error!("Agent 运行错误: {}", e);
            // 根据错误类型决定：重试 / 降级 / 通知用户
        }
    }
}
```

### 7. 多 Agent 架构

**不要创建巨大的单个 Agent** —— 将功能拆分为多个专注的 Agent，用 Workflow 组合：

```rust
// ✅ 好：职能分离
let coder = AgentBuilder::new("coder")      // 只写代码
    .instructions("你是 Rust 开发者。")
    .build()?;
let reviewer = AgentBuilder::new("reviewer") // 只审查代码
    .instructions("你是代码审查专家。")
    .build()?;

// 用 Workflow 组合
WorkflowBuilder::new()
    .node("coder", coder)
    .node("reviewer", reviewer)
    .connect("coder", "reviewer")
    .build_sequential("coder", "reviewer")?;
```

### 8. 性能优化

- **控制 `max_tool_rounds`** —— 默认 10 是合理的上限，避免无限循环。根据场景调整：
  - 简单工具链：3-5
  - 复杂操作：5-10
  - 最多不要超过 20
- **使用 `parallel_tool_calls`** —— 当 LLM 同时调用多个不相关工具时，框架并行执行：
  ```rust
  let options = AgentRunOptions::new()
      .with_properties([("parallel_tool_calls".into(), json!(true))]);
  ```
- **避免工具调用超长输出** —— `read_file` 有 512KB 限制，`run_command` 有 100KB 限制。如果工具可能产生大量输出，自行截断。

---

## 常见问题

### Q: Agent 为什么一直调用工具不停止？

A: LLM 可能陷入了工具调用循环。检查以下几点：
- `max_tool_rounds` 是否设置合理（默认 10）
- 工具的 description 是否准确描述了工具的功能，避免 LLM 误用
- 工具的返回值是否有明确的错误信息，让 LLM 知道调用失败

### Q: 如何让同一个工具在不同场景下有不同行为？

A: 创建多个 Agent 实例，每个实例注册不同策略的工具：

```rust
let read_only_agent = AgentBuilder::new("reader")
    .with_tool(ReadFile)       // 只有读权限
    .build()?;

let write_agent = AgentBuilder::new("writer")
    .with_tool(ReadFile)
    .with_tool(WriteFile)      // 有写权限
    .with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand))) // 但命令需审批
    .build()?;
```

### Q: Session 中的消息会无限增长吗？

A: 是的。在生产环境中建议：
1. 配置压缩策略（`SlidingWindowStrategy`）自动截断旧消息
2. 使用 `ISessionStore` 持久化到文件/数据库
3. 定期清理过期 Session

### Q: 为什么 `#[tool]` 宏不能用于带 `self` 的方法？

A: `#[tool]` 宏当前只支持独立的 async 函数。如果需要方法（例如带数据库连接），请手动实现 `ITool` trait。

### Q: 审批暂停后，如果用户关闭了应用怎么办？

A: Session 中已保存了 `assistant(tool_calls)` 消息。下次加载 Session 后，传入 `options.tool_approval_responses` 即可恢复。

### Q: 框架支持哪些 LLM 提供商？

A: 当前内置支持：
- **DeepSeek**：`DeepSeekChatClient`，支持 thinking 模式
- **OpenAI**：`OpenAIChatClient`，兼容 Azure OpenAI

扩展新的提供商：实现 `IChatClient` trait（3 个方法）。

---

## 其他导出类型

| 类型 | 位置 | 说明 |
|---|---|---|
| `ChatClientAgent` | `agent/` | 核心 Agent 实现，组合 ChatClient + Tools + ContextProviders |
| `PerServiceCallPersistingChatClient` | `decorators/` | ChatClient 装饰器，每次 LLM 调用后自动持久化 Session |
| `AgentResponseConverter` | `converter.rs` | 将内部 `AgentResponseUpdate` 流转换为 `AgentResponseResult` 流 |
| `InMemorySessionStore` | `session/` | 基于 `HashMap` 的内存 Session 存储，支持 TTL 自动清理 |
| `FileSystemSessionStore` | `session/` | 基于文件系统的 Session 持久化存储 |
| `IsolationScopedSessionStore` | `session/` | 基于租户/隔离键的 Session 存储包装器 |
| `IIsolationKeyProvider` | `session/` | 隔离键提供者 trait |
| `FixedIsolationKeyProvider` | `session/` | 固定隔离键的实现 |
| `EstimateCounter` | `token/` | 基于字符数的简易 Token 计数器（默认） |

---

## 依赖关系

```
rust-agent-framework
├── rust-agent-core       (traits, types, streaming)
├── rust-agent-macros     (#[tool] proc-macro, re-exported)
├── regex / glob / walkdir (file search tools)
├── dirs-next             (config directory resolution)
├── chrono                (timestamps)
├── tokio / futures       (async runtime)
├── tracing               (structured logging)
├── serde / serde_json    (serialization)
└── tiktoken-rs           (optional: accurate token counting)

被依赖:
├── rust-agent-cli        (交互式 CLI)
├── rust-agent-workflow   (工作流编排)
├── rust-agent-rhai       (Rhai 脚本集成)
└── rust-agent-decl       (声明式配置)
```

# rust-agent-websearch

`rust-agent-websearch` 是 [Rust Agent Framework (RAF)](https://gitcode.com/rf2026/rust-agent-framework) 的 **Web 搜索与网页抓取 Agent 工具集成** crate，提供开箱即用的 `web_search` 和 `web_fetch` 工具，以及配套的 `WebSearchContextProvider` 上下文提供器，支持零配置快速集成到任意 RAF Agent 中。

## 目录

- [功能概览](#功能概览)
- [安装](#安装)
- [快速开始](#快速开始)
  - [方式一：直接注册工具](#方式一直接注册工具)
  - [方式二：ContextProvider 集成（推荐）](#方式二contextprovider-集成推荐)
  - [方式三：自动搜索模式](#方式三自动搜索模式)
- [API 参考](#api-参考)
  - [WebSearch 工具](#websearch-工具)
  - [WebFetch 工具](#webfetch-工具)
  - [WebSearchContextProvider](#websearchcontextprovider)
- [环境变量](#环境变量)
- [配置优先级](#配置优先级)
- [错误处理与智能建议](#错误处理与智能建议)
- [搜索后端](#搜索后端)
- [架构与设计](#架构与设计)
- [示例](#示例)

## 功能概览

| 组件 | 类型 | 说明 |
|------|------|------|
| `WebSearch` | `#[tool]` | 多后端 Web 搜索引擎（DuckDuckGo / Bing / SearXNG），无需 API Key |
| `WebFetch` | `#[tool]` | 内嵌 Servo 浏览器引擎的网页抓取器，支持 JavaScript 渲染、SPA 水合、中文编码 |
| `WebSearchContextProvider` | `IContextProvider` | 上下文工程核心组件，自动注入搜索能力到 Agent，支持自动搜索模式 |
| `register_all()` | 函数 | 一键注册所有工具到 `ToolRegistry` |

## 安装

在 `Cargo.toml` 中添加依赖：

```toml
[dependencies]
rust-agent-websearch = "0.1.0"
```

本 crate 依赖 RAF 框架的核心类型，因此通常配合 `rust-agent-framework` 一起使用：

```toml
[dependencies]
rust-agent-framework = "0.1.0"
rust-agent-websearch = "0.1.0"
```

## 快速开始

`rust-agent-websearch` 提供三种集成方式，按推荐程度排序。

### 方式一：直接注册工具

最简模式，将 `WebSearch` 和 `WebFetch` 作为普通工具注册到 `ToolRegistry`：

```rust
use rust_agent_core::ToolRegistry;
use rust_agent_websearch::{WebSearch, WebFetch};

let mut registry = ToolRegistry::new();
registry.register(WebSearch);
registry.register(WebFetch);

// 或者一行搞定
rust_agent_websearch::register_all(&mut registry);
```

然后在构造 Agent 时传入：

```rust
use rust_agent_framework::AgentBuilder;
use rust_agent_websearch::register_all;

let mut registry = ToolRegistry::new();
register_all(&mut registry);

let agent = AgentBuilder::new("web-assistant")
    .chat_client(client)
    .instructions("你是一个可以帮助用户搜索和浏览网页的助手。")
    .with_tool(WebSearch)
    .with_tool(WebFetch)
    .build()?;
```

### 方式二：ContextProvider 集成（推荐）

使用 `WebSearchContextProvider` 将搜索能力作为上下文注入到 Agent 中。这种方式符合 RAF 的分层 ContextProvider 设计，自动注入工具声明和使用指引，并支持自动搜索模式。

```rust
use rust_agent_framework::AgentBuilder;
use rust_agent_websearch::WebSearchContextProvider;

let agent = AgentBuilder::new("research-agent")
    .chat_client(client)
    .instructions("你是一位研究助手，可以搜索互联网获取最新信息。")
    .add_context_provider(
        WebSearchContextProvider::new()
            .with_language("zh-CN")
    )
    .build()?;
```

**工作原理：**

1. `on_invoking()` 阶段自动向 Agent 注入：
   - **Instructions**: 工具使用说明（`## Web Search Capability` 块），引导 LLM 理解何时及如何使用搜索和抓取
   - **Tools**: `web_search` 和 `web_fetch` 两个工具（通过 `FnTool` 闭包实现，自包含无依赖）

2. `on_invoked()` 阶段无操作（错误容忍设计，不影响主流程）。

**与直接注册工具的区别：**

| 维度 | ToolRegistry 注册 | ContextProvider 注入 |
|------|-------------------|----------------------|
| 工具声明 | 需手动添加到 registry | 自动注入 |
| System 指令 | 需手动编写工具使用指引 | 自动生成广告文本 |
| 自动搜索 | 不支持 | 支持（开启后自动搜索） |
| 配置灵活性 | 受限于 `#[tool]` 环境变量 | 支持 Builder 模式链式配置 |
| 与框架耦合 | 松耦合 | 遵循 `IContextProvider` 标准，与其他 Provider 协同工作 |

### 方式三：自动搜索模式

启用自动搜索后，每次 Agent 调用前会自动提取最新用户消息作为搜索查询，将搜索结果以结构化 Markdown 注入到上下文中。Agent 无需主动调用 `web_search` 工具即可获得相关网页信息的预览。

```rust
use rust_agent_framework::AgentBuilder;
use rust_agent_websearch::WebSearchContextProvider;

let agent = AgentBuilder::new("auto-research-agent")
    .chat_client(client)
    .add_context_provider(
        WebSearchContextProvider::new()
            .with_auto_search(true)
            .with_max_results(5)
            .with_language("zh-CN")
    )
    .build()?;
```

**自动搜索流程：**

```
用户消息 → 提取最新 User message → 作为搜索查询 → 搜索 → 格式化 Markdown → 注入到 system instructions
```

注入格式示例：

```markdown
## Web Search Results for: "Rust 异步编程"

Found 5 result(s):

### [1.] Async Programming in Rust - Rust Documentation
- **URL**: https://rust-lang.org/async-book
- **Snippet**: Learn about async programming in Rust with the official...

---
*Tip: Use web_fetch(url) to get full content from any URL above.*
```

**注意事项：**

- 自动搜索会在每次 Agent 调用时产生额外的网络请求，增加响应延迟
- 结果超过 3000 字符会自动截断
- 搜索失败不影响 Agent 正常运行（静默降级）
- 建议配合 `tool_choice` 或 prompt 引导 Agent 在结果不足时主动使用 `web_search` 工具重新搜索

## API 参考

### WebSearch 工具

`WebSearch` 是通过 `#[tool]` 宏生成的 Agent 工具，实现 `ITool` trait。

**工具名称:** `web_search`

**参数：**

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `query` | `string` | 是 | - | 搜索关键词 |
| `count` | `integer` (optional) | 否 | `5` | 返回结果数量（最大 10） |

**返回值（JSON）：**

成功时：

```json
{
  "ok": true,
  "data": {
    "query": "Rust 编程",
    "results": [
      {
        "title": "结果标题",
        "url": "https://example.com",
        "snippet": "页面摘要...",
        "rank": 1
      }
    ],
    "count": 5,
    "_source": "DuckDuckGoLite",
    "_fingerprint": 12345678901234567890,
    "_tip": "Use web_fetch(url) to get full content..."
  }
}
```

失败时：

```json
{
  "ok": false,
  "data": null,
  "error": "Search failed: Rate limited",
  "suggestion": "Search rate limited. Wait a moment and try again."
}
```

**直接调用（用于测试/调试）：**

```rust
use rust_agent_websearch::WebSearch;

let raw = WebSearch.call("Rust 编程语言".to_string(), Some(5)).await;
let result: serde_json::Value = serde_json::from_str(&raw)?;
```

### WebFetch 工具

`WebFetch` 是通过 `#[tool]` 宏生成的网页抓取工具，内嵌 Servo 浏览器引擎实现真实 DOM 渲染和 JavaScript 执行。

**工具名称:** `web_fetch`

**参数：**

| 参数名 | 类型 | 必填 | 默认值 | 说明 |
|--------|------|------|--------|------|
| `url` | `string` | 是 | - | 要抓取的 URL |
| `max_length` | `integer` (optional) | 否 | `50000` | 最大内容长度（字节），范围 1000-200000 |
| `settle_ms` | `integer` (optional) | 否 | `0` | SPA 页面水合等待时间（毫秒），最大 10000 |

**返回值（JSON）：**

成功时：

```json
{
  "ok": true,
  "data": {
    "url": "https://example.com",
    "final_url": "https://example.com/page",
    "title": "Page Title",
    "content": "# Page Title\n\nMarkdown content...",
    "content_length": 12345,
    "truncated": false,
    "status_code": 200
  }
}
```

**特性：**

- **Servo 浏览器引擎**：真实的 HTML/CSS 解析和 DOM 构建，非正则提取
- **JavaScript 执行**：支持现代 SPA 应用的动态渲染
- **布局感知提取**：自动去除导航栏、页脚、Cookie 弹窗、广告
- **中文编码支持**：GBK / GB2312 / Big5 自动检测和转换
- **安全防护**：SSRF 防护，阻止内网 IP 和保留地址访问
- **内容截断保护**：超长内容自动截断并标记

### WebSearchContextProvider

`WebSearchContextProvider` 实现了 `IContextProvider` trait，是 RAF 上下文工程体系的核心组件。

**构造器：**

```rust
let provider = WebSearchContextProvider::new();
```

**Builder 方法（链式调用）：**

| 方法 | 参数 | 说明 |
|------|------|------|
| `with_auto_search(enabled: bool)` | `true`/`false` | 启用/禁用自动搜索（默认 `false`） |
| `with_max_results(max: usize)` | 1-10 | 自动搜索的最大结果数（默认 5） |
| `with_proxy(url: impl Into<String>)` | 代理 URL | HTTP/SOCKS5 代理（覆盖环境变量） |
| `with_searxng(url: impl Into<String>)` | SearXNG 地址 | SearXNG 实例地址（覆盖环境变量） |
| `with_language(lang: impl Into<String>)` | 语言代码 | 搜索语言偏好（如 `"zh-CN"`、`"en-US"`） |

**完整配置示例：**

```rust
let provider = WebSearchContextProvider::new()
    .with_auto_search(true)
    .with_max_results(8)
    .with_proxy("socks5://127.0.0.1:1080")
    .with_searxng("https://searx.example.com")
    .with_language("zh-CN");
```

**IContextProvider 生命周期：**

```
Agent.run() 调用
  │
  ├─ Phase 1: on_invoking(agent, session, messages, options)
  │   ├─ 注入 Web Search 能力声明（advertise）到 system instructions
  │   ├─ 注入 web_search / web_fetch 工具到可用工具列表
  │   └─ [如果 auto_search=true] 执行自动搜索并注入结果
  │
  ├─ Phase 2: LLM 调用（Agent 可使用注入的工具进行搜索/抓取）
  │
  └─ Phase 3: on_invoked(agent, session, request_messages, response, error)
      └─ 无操作（WebSearchContextProvider 不需要后处理）
```

## 环境变量

| 变量名 | 说明 | 示例 |
|--------|------|------|
| `WEBSEARCH_PROXY_URL` | HTTP/SOCKS5 代理地址 | `http://127.0.0.1:7890` 或 `socks5://127.0.0.1:1080` |
| `WEBSEARCH_SEARXNG_URL` | 自建 SearXNG 实例地址 | `https://searx.example.com` |

## 配置优先级

配置项的优先级从高到低：

1. **Builder 方法** — `WebSearchContextProvider::new().with_proxy(...)` 等
2. **环境变量** — `WEBSEARCH_PROXY_URL`、`WEBSEARCH_SEARXNG_URL`
3. **框架默认值** — 如 `max_results = 5`

这一设计允许你在代码中硬编码生产环境配置，同时通过环境变量在不同部署环境中灵活覆盖。

## 错误处理与智能建议

`web_search` 和 `web_fetch` 工具**不会返回 Rust 级别错误**（即不抛 `AgentError`），而是返回结构化的失败结果，由 LLM 自行判断是否重试。这样设计是为了：

- **不影响 Agent 主流程**：一次搜索失败不会中断整个 Agent 运行
- **智能建议**：根据错误类型提供可操作的指引，帮助 LLM 做出更好的决策
- **语义提示**：错误消息面向 LLM 可读，而非面向开发者

**错误示例：**

```json
{
  "ok": false,
  "data": null,
  "error": "Search failed: Rate limited",
  "suggestion": "Search rate limited. Wait a moment and try again, or use a different query phrasing."
}
```

## 搜索后端

本 crate 底层的 `rust-websearch` 库使用**多后端智能选择**策略：

```
1. DuckDuckGo Lite (lite.duckduckgo.com)      ← 首选，零配置
2. DuckDuckGo HTML (html.duckduckgo.com)       ← 备选
3. Bing CN (cn.bing.com)                       ← 国内可达
4. DuckDuckGo Instant Answer API               ← 结构化结果
5. SearXNG 自建实例                            ← 需配置
```

在网络受限地区，可以通过设置 `WEBSEARCH_SEARXNG_URL` 或配置代理来确保搜索可用。

## 架构与设计

```
┌─────────────────────────────────────────────────────┐
│                  rust-agent-websearch                │
├─────────────────────────────────────────────────────┤
│  ┌──────────────────┐  ┌─────────────────────────┐  │
│  │   WebSearch       │  │  WebSearchContextProvider │  │
│  │   (#[tool])       │  │  (IContextProvider)       │  │
│  │                   │  │                           │  │
│  │  • web_search()   │  │  • with_auto_search()     │  │
│  │    query, count   │  │  • with_max_results()     │  │
│  │                   │  │  • with_proxy()           │  │
│  ├──────────────────┤  │  • with_searxng()         │  │
│  │   WebFetch        │  │  • with_language()        │  │
│  │   (#[tool])       │  │                           │  │
│  │                   │  │  on_invoking() ──────────►│  │
│  │  • web_fetch()    │  │    → instructions (ad)    │  │
│  │    url, max_len,  │  │    → tools (2)            │  │
│  │    settle_ms      │  │    → auto_search results  │  │
│  └──────────────────┘  └──────────┬────────────────┘  │
├───────────────────────────────────┼───────────────────┤
│                                   ▼                    │
│                        rust-websearch                  │
│              (多后端搜索引擎 + Servo 抓取)              │
└─────────────────────────────────────────────────────┘
```

**设计原则：**

- **零 API Key**：所有搜索后端和网页抓取均无需注册 API Key
- **自包含工具**：`WebSearchContextProvider` 通过内部 `FnTool` 实现工具，不依赖 `#[tool]` 宏生成的 `WebSearch`/`WebFetch` 结构体，避免循环依赖
- **分层注入**：遵循 RAF 的 ContextProvider 分层设计（Agent 层一次性注入），保证 KV Cache 前缀稳定性
- **静默降级**：网络失败、搜索无结果等均不影响 Agent 主流程，LLM 根据结构化错误信息自行决策

## 示例

### 命令行示例：搜索并抓取

运行内置的 CLI 示例，直接体验搜索和抓取流程：

```bash
# 基本用法：搜索 "Rust编程语言"，返回 3 条结果，抓取第 1 条
cargo run -p rust-agent-websearch --example search_and_fetch -- Rust编程语言 3 0

# 搜索并抓取更多结果
cargo run -p rust-agent-websearch --example search_and_fetch -- "async Rust tutorial" 5 0
```

### Agent 集成示例

一个完整的 RAF Agent 集成，具备 Web 搜索、网页抓取和自动搜索能力：

```rust
use rust_agent_framework::AgentBuilder;
use rust_agent_websearch::WebSearchContextProvider;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 创建 LLM 客户端（以 DeepSeek 为例）
    let client = /* 你的 IChatClient 实现 */;

    // 2. 构建带 Web 搜索能力的 Agent
    let agent = AgentBuilder::new("web-researcher")
        .chat_client(client)
        .instructions(concat!(
            "你是一位研究助手，可以帮助用户通过搜索互联网获取最新信息。\n",
            "工作流程：\n",
            "1. 当用户询问需要最新信息的问题时，使用 web_search 搜索\n",
            "2. 从搜索结果中选择最相关的 URL\n",
            "3. 使用 web_fetch 获取完整页面内容\n",
            "4. 基于获取的内容给出答案，并引用来源\n",
        ))
        .add_context_provider(
            WebSearchContextProvider::new()
                .with_auto_search(true)  // 可选：启用自动搜索预填充上下文
                .with_max_results(5)
                .with_language("zh-CN")
        )
        .build()?;

    // 3. 使用 Agent 进行对话
    let session = agent.create_session();
    let messages = vec![rust_agent_core::ChatMessage::user(
        "Rust 语言最新的异步运行时有哪些？"
    )];

    let mut stream = agent.run(messages, Some(session), None).await?;
    // 处理流式响应...
    // 如果启用了 auto_search，Agent 在回答前已经预加载了相关搜索结果的上下文

    Ok(())
}
```

### 仅使用工具（最小化集成）

如果不需要 ContextProvider 的自动注入，也可以直接使用工具：

```rust
use rust_agent_core::ToolRegistry;
use rust_agent_websearch::{WebSearch, WebFetch};
use std::sync::Arc;

async fn manual_search_example() {
    // 直接调用工具
    let search_result = WebSearch.call(
        "Rust async runtime comparison 2025".to_string(),
        Some(5),
    ).await;

    let json: serde_json::Value = serde_json::from_str(&search_result).unwrap();
    if json["ok"] == true {
        let results = &json["data"]["results"];
        if let Some(first_url) = results[0]["url"].as_str() {
            // 抓取第一个结果的完整内容
            let page = WebFetch.call(
                first_url.to_string(),
                Some(50000),  // max_length
                None,         // settle_ms
            ).await;
            let page_json: serde_json::Value = serde_json::from_str(&page).unwrap();
            println!("标题: {}", page_json["data"]["title"]);
            println!("内容预览: {}...", &page_json["data"]["content"].as_str().unwrap_or("")[..200.min(
                page_json["data"]["content"].as_str().map(|s| s.len()).unwrap_or(0)
            )]);
        }
    }
}
```

### 多 Provider 组合

`WebSearchContextProvider` 可以与其他 ContextProvider 组合使用，Provider 按注册顺序执行：

```rust
use rust_agent_framework::AgentBuilder;
use rust_agent_websearch::WebSearchContextProvider;
use rust_agent_framework::context_providers::skills_provider::AgentSkillsProvider;

let agent = AgentBuilder::new("full-featured-assistant")
    .chat_client(client)
    .instructions("你是一个全能助手。")
    .add_context_provider(
        AgentSkillsProvider::scan("/path/to/skills")?  // 第 2 个：技能
    )
    .add_context_provider(
        WebSearchContextProvider::new()                // 第 3 个：Web 搜索
            .with_auto_search(true)
            .with_language("zh-CN")
    )
    .build()?;

// Provider 链: [InMemoryHistoryProvider, AgentSkillsProvider, WebSearchContextProvider]
// - InMemoryHistoryProvider: 加载对话历史
// - AgentSkillsProvider: 注入技能指令和工具
// - WebSearchContextProvider: 注入 Web 搜索能力
```

---

## License

MIT

# 基于 servo-fetch 全面重构 crates/websearch 计划

## 概述

使用 `servo-fetch` crate（v0.12.2）替代当前基于 `reqwest` + `scraper` + `encoding_rs` 的自建抓取/渲染/提取体系，全面重构 `crates/websearch` 的 fetcher 层和内容提取层。

**核心收益**：
- **真实 JS 执行**：SpiderMonkey 引擎渲染 SPA/动态页面，解决当前 fetcher 对 JS 渲染页面无能为力的问题
- **布局感知提取**：基于渲染位置剥离导航栏/页脚/Cookie 横幅等，远优于当前基于 HTML 猜测的 Readability 算法
- **Markdown 输出**：直接输出高质量可读 Markdown，替代当前粗糙的纯文本提取
- **Schema 驱动 JSON 提取**：声明式 CSS 选择器提取结构化数据，无需 LLM
- **SSRF 防护**：内置私有 IP/保留地址段拦截
- **PDF 自动检测**：URL 返回 PDF 时自动提取文本

## 当前状态分析

### 当前架构（将被重构的部分）

| 模块 | 文件 | 职责 | 依赖 |
|---|---|---|---|
| `fetcher` | `fetcher.rs` | HTTP GET + 编码检测 + 正文提取 | reqwest, encoding_rs, content_extractor |
| `content_extractor` | `content_extractor.rs` | Readability 风格正文提取（评分法/选择器/body 降级） | scraper |
| `encoding` | `encoding.rs` | 5 级编码检测（BOM → 声明 → UTF-8 → 中文编码 → lossy） | encoding_rs |
| `html_utils` | `html_utils.rs` | HTML 清理工具 | scraper |
| `anti_detection` | `anti_detection/` | UA 轮换、速率限制、Cookie 管理、代理管理 | reqwest, cookie_store, rand |

### 保持不变的部分

| 模块 | 文件 | 原因 |
|---|---|---|
| `searcher` | `searcher.rs` | 搜索协调逻辑，与 fetch 层解耦 |
| `duckduckgo/` | 3 个文件 | 搜索引擎 HTML/API 解析，使用 reqwest 发请求 |
| `bing` | `bing.rs` | 同上 |
| `searxng` | `searxng.rs` | 同上 |
| `probe` | `probe.rs` | 网络探测，使用 reqwest |
| `types` | `types.rs` | 部分类型需调整，但搜索相关类型不变 |
| `error` | `error.rs` | 需扩展，但基础结构保留 |

### servo-fetch API 映射

| 当前功能 | servo-fetch 替代 |
|---|---|
| `fetch_page()` + 编码检测 + 正文提取 | `servo_fetch::markdown(url)` 或 `servo_fetch::fetch(FetchOptions)` |
| `FetchConfig` (timeout, max_content_bytes, proxy) | `FetchOptions::new(url).timeout(Duration).user_agent(ua)` |
| `FetchedPage` (url, final_url, title, content, status_code) | `Page` (html, markdown(), title, url, status_code 等) |
| `content_extractor::extract_main_content()` | `Page::markdown()` / `Page::text_content` |
| `encoding::decode_bytes()` | servo-fetch 内部处理（Servo 引擎自带编码支持） |
| UA 轮换 | `FetchOptions::user_agent()` / `SERVO_FETCH_USER_AGENT` 环境变量 |
| 速率限制 | 保留自建 `RateLimiter`（servo-fetch 无此功能） |
| Cookie 管理 | `servo_fetch::load_cookies()` + `FetchOptions::cookies()` |
| 代理 | servo-fetch 不直接支持代理配置，需通过环境变量或网络层 |

## 提议变更

### 1. 更新 `Cargo.toml` 依赖

**文件**: `crates/websearch/Cargo.toml`

- **新增**: `servo-fetch = "0.12"`
- **移除**: `scraper`, `encoding_rs`, `cookie_store`（这些功能由 servo-fetch 内部覆盖）
- **保留**: `reqwest`（搜索后端仍需使用）、`tokio`、`serde`/`serde_json`、`tracing`、`futures-util`、`rand`、`url`

注意：servo-fetch v0.12.2 的 API 是同步的（非 async），需要通过 `tokio::task::spawn_blocking` 包装为异步调用。

### 2. 重构 `fetcher.rs` — 核心变更

**文件**: `crates/websearch/src/fetcher.rs`

将 `fetch_page()` 从基于 reqwest 的 HTTP GET + 手动编码检测 + 自建正文提取，改为基于 `servo_fetch::fetch()` 的浏览器级渲染提取。

```rust
// 新实现骨架
pub async fn fetch_page(url: &str, config: &FetchConfig) -> Result<FetchedPage, SearchError> {
    // 1. 速率控制（保留自建）
    fetch_rate_limiter().wait(config.min_interval_ms).await;

    // 2. 构建 servo-fetch FetchOptions
    let mut opts = servo_fetch::FetchOptions::new(url)
        .timeout(Duration::from_secs(config.timeout_secs));

    if let Some(ref ua) = config.user_agent {
        opts = opts.user_agent(ua);
    }

    // 3. 在 spawn_blocking 中执行同步 servo-fetch 调用
    let page = tokio::task::spawn_blocking(move || servo_fetch::fetch(&opts))
        .await
        .map_err(|e| SearchError::Other(format!("spawn_blocking error: {e}")))?
        .map_err(|e| Search_error_from_servo(e))?;

    // 4. 提取内容（优先 Markdown，降级纯文本）
    let content = page.markdown()
        .unwrap_or_else(|_| page.text_content.clone().unwrap_or_default());

    // 5. 截断处理
    let (content, truncated) = truncate_content(&content, config.max_content_bytes);

    Ok(FetchedPage {
        url: url.to_string(),
        final_url: page.url.clone().unwrap_or_else(|| url.to_string()),
        title: page.title.clone().unwrap_or_default(),
        content,
        content_length: content.len(),
        truncated,
        status_code: page.status_code.unwrap_or(200),
    })
}
```

### 3. 删除 `content_extractor.rs`

**文件**: `crates/websearch/src/content_extractor.rs` → **删除**

servo-fetch 的 `Page::markdown()` 提供了基于渲染位置的布局感知提取，远优于自建的 Readability 评分算法。此模块完全不再需要。

### 4. 删除 `encoding.rs`

**文件**: `crates/websearch/src/encoding.rs` → **删除**

Servo 引擎内置完整的编码支持，自动处理 GBK/GB2312/Big5 等编码，无需手动编码检测。

同时删除 `lib.rs` 中的 `pub mod encoding` 和相关 re-export。

### 5. 简化 `html_utils.rs`

**文件**: `crates/websearch/src/html_utils.rs`

- 保留搜索后端解析所需的 HTML 工具函数（如 `clean_html`、`resolve_duckduckgo_url`）
- 移除仅服务于 `content_extractor` 的函数

### 6. 更新 `types.rs`

**文件**: `crates/websearch/src/types.rs`

- `FetchConfig` 新增 `user_agent: Option<String>` 和 `settle_ms: Option<u64>`（SPA 等待时间）字段
- `FetchConfig` 移除 `proxy_url`（servo-fetch 不直接支持代理，改用环境变量）
- `FetchedPage` 保持兼容，字段不变

### 7. 更新 `error.rs`

**文件**: `crates/websearch/src/error.rs`

- 新增 `From<servo_fetch::Error> for SearchError` 实现
- 映射 servo-fetch 错误类型：`Error::Timeout` → `SearchError::Timeout`、`Error::InvalidUrl` → `SearchError::Config` 等

### 8. 简化 `anti_detection/` 模块

**文件**: `crates/websearch/src/anti_detection/`

- **保留**: `rate_limiter.rs`（servo-fetch 无速率限制功能）、`user_agent.rs`（用于搜索后端和 fetcher 的 UA 轮换）
- **移除**: `cookie_mgr.rs`（servo-fetch 自带 Cookie 管理）、`proxy.rs`（servo-fetch 通过环境变量处理代理）
- **更新**: `mod.rs` 中的 `build_client()` 保留（搜索后端仍需 reqwest 客户端），新增 `build_servo_fetch_options()` 辅助函数

### 9. 更新 `lib.rs` 公共 API

**文件**: `crates/websearch/src/lib.rs`

- 移除 `pub mod content_extractor` 和 `pub mod encoding`
- 移除 `pub use content_extractor::extract_main_content`
- 移除 `pub use encoding::{decode_bytes, parse_content_type_charset, parse_meta_charset}`
- 新增 `pub use fetcher::fetch_page` 的增强版（保持签名兼容）

### 10. 更新下游 `websearch-ai` crate

**文件**: `crates/websearch-ai/src/web_fetch.rs`

- `WebFetch` 工具的 `fetch_page` 调用签名不变，但行为大幅增强：
  - 自动支持 JS 渲染页面
  - 输出 Markdown 格式（更结构化）
  - 移除 "JS 渲染页面可能为空" 的提示（servo-fetch 已解决此问题）
- 新增 `settle_ms` 参数，允许 Agent 等待 SPA 水合

## 假设与决策

1. **servo-fetch 同步 API**：v0.12.2 的核心 `fetch()` 是同步的，需用 `spawn_blocking` 包装。这会增加少量线程开销，但避免了复杂的异步运行时集成。

2. **代理支持**：servo-fetch 不直接支持 HTTP 代理配置。对于需要代理的场景，保留通过环境变量（`HTTP_PROXY`/`HTTPS_PROXY`）的方式，或在 `FetchConfig` 中保留 `proxy_url` 字段但标注为"仅搜索后端使用"。

3. **搜索后端不变**：DuckDuckGo/Bing/SearXNG 的搜索请求仍使用 reqwest，因为它们只需要简单的 HTTP 请求 + HTML/JSON 解析，不需要浏览器渲染。

4. **Feature flag**：新增 `servo-fetch` feature flag，允许在不需要浏览器渲染的环境（如轻量搜索场景）中禁用 servo-fetch 依赖，回退到 reqwest 方式。默认启用。

5. **平台兼容性**：servo-fetch 在 Linux 上需要 `libegl1`、`libfontconfig1`、`libfreetype6`，在 Windows 上需要 `libEGL.dll` 和 `libGLESv2.dll`。需要在文档中说明运行时依赖。

## 验证步骤

1. **编译验证**: `cargo build -p rust-websearch` 通过
2. **单元测试**: `cargo test -p rust-websearch` 通过
3. **集成测试**: `cargo test -p rust-agent-websearch` 通过
4. **功能验证**:
   - `fetch_page("https://example.com")` 返回有效 Markdown 内容
   - JS 渲染页面（如 SPA 站点）能正确提取内容
   - 中文站点（GBK 编码）能正确解码
   - 速率限制功能正常工作
   - 搜索功能（DuckDuckGo/Bing/SearXNG）不受影响
5. **全 workspace 编译**: `cargo build` 通过

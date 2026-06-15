# 纯 Rust 免 API Key 网络搜索库 —— 新建 `rust-agent-websearch` 专用 Crate

## 一、摘要

新建独立的 `rust-agent-websearch` crate（`crates/websearch/`），封装纯 Rust 实现、无需注册 API Key、具备反爬绕过能力的网络搜索库。该 crate 作为独立库可被任何 Rust 项目直接使用，同时被 `rust-agent-framework` 集成作为 Agent 工具。

---

## 二、现状分析

### 2.1 现有实现

| 文件 | 功能 | 技术方案 |
|---|---|---|
| [web_search.rs](file:///e:/GitCode/RF/rust-agent-framework/crates/framework/src/tools/web_search.rs) | 网页搜索 | ① `tarzi` 浏览器模式（需 ChromeDriver/GeckoDriver）→ ② DuckDuckGo HTML 降级 |
| [web_fetch.rs](file:///e:/GitCode/RF/rust-agent-framework/crates/framework/src/tools/web_fetch.rs) | 网页抓取 | `tarzi::WebFetcher`（浏览器模式） |

### 2.2 现有问题

1. **搜索逻辑与框架耦合**——搜索代码嵌入 `tools/` 目录，无法作为独立库复用
2. **`tarzi` 依赖外部浏览器**——ChromeDriver/GeckoDriver 在 Windows 上不可用
3. **反爬能力极弱**——仅设置 User-Agent 和 redirect policy，无 TLS 指纹、CAPTCHA 处理、代理支持
4. **DuckDuckGo 解析脆弱**——正则解析 HTML，引擎改版即失效
5. **搜索源单一**——只有 DuckDuckGo 一个回退源
6. **无速率控制**——连续请求易触发限流

### 2.3 Workspace 架构

当前 workspace 有 9 个 crate（[Cargo.toml](file:///e:/GitCode/RF/rust-agent-framework/Cargo.toml#L1-L12)），依赖层次：

```
core → client → macros → framework → cli/decl/rhai/workflow
```

新 crate 将位于 `clients/` 层（独立于框架核心），仅依赖基础库。

---

## 三、技术调研结论

### 3.1 免 API Key 搜索方案对比

| 方案 | 搜索质量 | 部署复杂度 | 稳定性 | 纯 Rust 可行性 |
|---|---|---|---|---|
| DuckDuckGo Lite（`lite.duckduckgo.com`） | 中 | 零配置 | 高 | 高 |
| DuckDuckGo Instant Answer（`api.duckduckgo.com`） | 低（仅知识类） | 零配置 | 高 | 高 |
| DuckDuckGo HTML（`html.duckduckgo.com`） | 中 | 零配置 | 中（可能 CAPTCHA） | 高 |
| SearXNG 自建实例 | 高（聚合 70+ 引擎） | 需 Docker | 高 | 高（客户端） |

**结论**：DuckDuckGo 最优零配置方案，SearXNG 最优高质量方案。两者互补。

### 3.2 纯 Rust 反爬技术矩阵

| 技术 | Rust 实现 | 效果 | 门槛 |
|---|---|---|---|
| User-Agent 池轮换 | 维护真实 UA 列表 | 基础 | 低 |
| CSS Selector 解析 | `scraper` crate（Servo 引擎） | 健壮 | 低 |
| 速率控制 + 随机抖动 | `tokio::time::sleep` + rand | 避免频控 | 低 |
| Cookie/Session 管理 | `cookie_store` crate | 维持会话 | 低 |
| 代理轮换 | HTTP/SOCKS5 代理池 | IP 级 | 中 |
| TLS 指纹伪装 | `wreq`（BoringSSL，JA4 ≈ Chrome） | 绕过 Cloudflare | 高（编译依赖） |
| CAPTCHA 交互求解 | `ddg-rs` 模式（下载 tile → 用户交互） | DuckDuckGo 验证码 | 中（需 TTY） |

### 3.3 关键依赖 Crate

| Crate | 版本 | 用途 | 许可证 |
|---|---|---|---|
| [scraper](https://crates.io/crates/scraper) | 0.22 | 浏览器级 HTML 解析（CSS Selector） | ISC |
| [cookie_store](https://crates.io/crates/cookie_store) | 0.22 | HTTP Cookie 持久化 | MIT/Apache-2.0 |
| [reqwest](https://crates.io/crates/reqwest) | 0.12 | HTTP 客户端（已工作区依赖） | MIT/Apache-2.0 |
| [rand](https://crates.io/crates/rand) | 0.8 | 随机数（UA 轮换、抖动） | MIT/Apache-2.0 |
| [serde](https://crates.io/crates/serde) / [serde_json](https://crates.io/crates/serde_json) | 1 | 序列化（已工作区依赖） | MIT/Apache-2.0 |
| [tokio](https://crates.io/crates/tokio) | 1 | 异步运行时（已工作区依赖） | MIT |
| [wreq](https://crates.io/crates/wreq) | - | TLS 指纹伪装（可选 feature） | - |

---

## 四、方案设计

### 4.1 架构定位

```
crates/websearch/          ← 新建：独立通用库
  ↑ 依赖
crates/framework/          ← 集成：封装为 #[tool] 工具
```

新 crate 提供纯数据结构和功能函数，不依赖 `rust-agent-core`、`rust-agent-macros` 等框架内部库，可被任意 Rust 项目独立使用。

### 4.2 降级搜索链路

```
search_lite()  → DuckDuckGo Lite（首选，纯 HTTP，最稳定）
    ↓ 失败
search_instant_answer()  → DuckDuckGo Instant Answer（JSON API）
    ↓ 失败
search_html()  → DuckDuckGo HTML（增强版 CSS 解析）
    ↓ 失败（可选，需用户配置）
search_searxng()  → 自建/公共 SearXNG 实例
```

### 4.3 反爬增强策略

| 优先级 | 策略 | 实现 |
|---|---|---|
| **P0** | User-Agent 池轮换 | 10+ 主流 UA，每次随机选取 |
| **P0** | CSS Selector 替代正则 | `scraper` crate 解析 HTML |
| **P0** | 速率控制 + 随机抖动 | 可配置间隔 + ±30% 随机抖动 |
| **P1** | Cookie 管理 | `cookie_store` 自动管理会话 |
| **P1** | DuckDuckGo Lite 端点 | 最轻量、最少反爬 |
| **P1** | 代理支持 | HTTP/SOCKS5 代理 + 轮换 |
| **P2** | TLS 指纹伪装 | 可选 feature `tls-stealth`（集成 `wreq`） |
| **P2** | CAPTCHA 检测 + 交互求解 | 检测 202 → 下载 tile → TTY 交互提交 |
| **P2** | SearXNG 公共实例 | 内置列表 + 健康检查 |

### 4.4 Public API 设计

```rust
// ── 核心类型 ──

/// 单条搜索结果
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: SearchSource,   // 来源引擎
    pub rank: usize,
}

/// 搜索结果集合
pub struct SearchResults {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub source: SearchSource,
}

/// 搜索来源
pub enum SearchSource {
    DuckDuckGoLite,
    DuckDuckGoHtml,
    DuckDuckGoInstantAnswer,
    SearXNG,
}

/// 搜索配置
pub struct SearchConfig {
    pub max_results: usize,
    pub timeout_secs: u64,
    pub min_interval_ms: u64,    // 最小请求间隔
    pub proxy_url: Option<String>,
    pub searxng_url: Option<String>,
    pub language: Option<String>,
}

// ── 核心函数 ──

/// 搜索（自动选择最佳后端，按降级链依次尝试）
pub async fn search(query: &str, config: &SearchConfig) -> Result<SearchResults, SearchError>;

/// 仅 DuckDuckGo Lite
pub async fn search_lite(query: &str, config: &SearchConfig) -> Result<SearchResults, SearchError>;

/// 仅 DuckDuckGo HTML
pub async fn search_html(query: &str, config: &SearchConfig) -> Result<SearchResults, SearchError>;

/// 仅 DuckDuckGo Instant Answer
pub async fn search_instant_answer(query: &str, config: &SearchConfig) -> Result<SearchResults, SearchError>;

/// 仅 SearXNG
pub async fn search_searxng(query: &str, config: &SearchConfig) -> Result<SearchResults, SearchError>;

/// 网页内容抓取（HTML → Markdown/纯文本）
pub async fn fetch_page(url: &str, config: &FetchConfig) -> Result<FetchedPage, SearchError>;
```

---

## 五、实施计划

### Phase 1：创建 Crate 骨架

**新建 `crates/websearch/` 目录结构：**

```
crates/websearch/
├── Cargo.toml
└── src/
    ├── lib.rs              # crate 入口，重新导出所有 public API
    ├── error.rs            # SearchError 错误类型
    ├── types.rs            # SearchResult, SearchConfig, FetchConfig 等核心类型
    ├── searcher.rs         # 顶层 search() 协调函数（降级链）
    ├── duckduckgo/
    │   ├── mod.rs          # DuckDuckGo 模块入口
    │   ├── lite.rs         # DuckDuckGo Lite 后端（lite.duckduckgo.com）
    │   ├── html.rs         # DuckDuckGo HTML 后端增强版
    │   └── instant_answer.rs  # DuckDuckGo Instant Answer API
    ├── searxng.rs          # SearXNG 客户端
    ├── fetcher.rs          # 网页内容抓取（替代 tarzi::WebFetcher）
    ├── anti_detection/
    │   ├── mod.rs          # 反爬模块入口
    │   ├── user_agent.rs   # UA 池与随机轮换
    │   ├── rate_limiter.rs # 速率控制 + 抖动
    │   ├── cookie_mgr.rs   # Cookie 管理器
    │   └── proxy.rs        # 代理管理器
    └── html_utils.rs       # HTML 解码、清理等工具函数（从 web_search.rs 迁移）
```

**修改文件：**
- [Cargo.toml](file:///e:/GitCode/RF/rust-agent-framework/Cargo.toml) —— 添加 `crates/websearch` 到 workspace members，添加 workspace dependency
- [Cargo.toml](file:///e:/GitCode/RF/rust-agent-framework/crates/framework/Cargo.toml) —— 添加 `rust-agent-websearch` 依赖，移除 `tarzi`

### Phase 2：实现 DuckDuckGo 三后端搜索

1. **DuckDuckGo Lite**（`lite.duckduckgo.com`）
   - 纯 HTTP GET，返回极简 HTML
   - 使用 `scraper` crate CSS 选择器解析
   - 最不容易触发 CAPTCHA，首选后端

2. **DuckDuckGo Instant Answer**（`api.duckduckgo.com`）
   - JSON API，解析 `Abstract`、`AbstractURL`、`RelatedTopics`、`Infobox`
   - 适合知识类、定义类查询
   - 结果数量有限（非通用网页搜索）

3. **DuckDuckGo HTML 增强**（`html.duckduckgo.com`）
   - 用 `scraper` 替代现有正则解析
   - CSS 选择器：`a.result__a` (title+url)、`a.result__snippet` (snippet)
   - 结果去重、URL 跳转解析（`/l/?uddg=...`）

4. **降级协调器**（`searcher.rs`）
   - `search()` 函数按 Lite → Instant Answer → HTML 依次尝试
   - 每个后端失败时记录 warning 日志
   - 全部失败返回详细 `SearchError`

### Phase 3：实现反爬模块

1. **UA 池**（`anti_detection/user_agent.rs`）
   - 常量数组维护 Chrome 130+、Edge 130+、Firefox 130+ 桌面版 UA
   - `fn random_user_agent() -> &'static str`

2. **速率限制器**（`anti_detection/rate_limiter.rs`）
   - `struct RateLimiter`（内部用 `Mutex<Option<Instant>>`）
   - `async fn wait(&self, min_interval_ms: u64)` —— 确保间隔 + 随机 jitter（±30%）

3. **Cookie 管理器**（`anti_detection/cookie_mgr.rs`）
   - 包装 `cookie_store::CookieStore`
   - 与 `reqwest` 集成：请求前注入 Cookie → 响应后提取 Set-Cookie
   - 按域名隔离

4. **代理管理器**（`anti_detection/proxy.rs`）
   - `struct ProxyManager`：管理代理列表
   - 支持 HTTP/HTTPS/SOCKS5 代理
   - 轮换策略：Round-Robin + 失败自动摘除

### Phase 4：实现网页抓取 & SearXNG

1. **网页抓取**（`fetcher.rs`）
   - 替代 `tarzi::WebFetcher`，纯 `reqwest` HTTP 实现
   - 提取 HTML `<title>` 和正文内容
   - 简单 HTML → 纯文本转换（去除 script/style 标签）
   - 内容截断（默认 50KB）
   - 复用反爬模块（UA、速率、Cookie、代理）

2. **SearXNG 客户端**（`searxng.rs`）
   - 调用 SearXNG JSON API（`/search?q=...&format=json`）
   - 支持 categories、engines、language、time_range 参数
   - 内置公共实例列表 + 健康检查 + 自动故障切换

### Phase 5：集成到框架

1. **修改 framework Cargo.toml**
   - 添加 `rust-agent-websearch = { workspace = true }`
   - 移除 `tarzi = "0.1"`

2. **重构 [web_search.rs](file:///e:/GitCode/RF/rust-agent-framework/crates/framework/src/tools/web_search.rs)**
   - 移除内联搜索函数（`search_via_tarzi`、`search_via_duckduckgo`、`parse_duckduckgo_results` 等）
   - 代理调用到 `rust_agent_websearch::search()`
   - `#[tool]` 接口保持不变（参数、返回 JSON 格式不变）

3. **重构 [web_fetch.rs](file:///e:/GitCode/RF/rust-agent-framework/crates/framework/src/tools/web_fetch.rs)**
   - 移除 `tarzi::WebFetcher` 调用
   - 代理到 `rust_agent_websearch::fetch_page()`

4. **更新 [mod.rs](file:///e:/GitCode/RF/rust-agent-framework/crates/framework/src/tools/mod.rs)**
   - 保持 re-export 和 `register_all()` 不变

5. **可选 feature 支持 —— TLS 指纹伪装**
   - `rust-agent-websearch` 提供 Cargo feature `tls-stealth`
   - 启用时使用 `wreq` 替代 `reqwest`（BoringSSL JA4 指纹 ≈ Chrome）
   - 默认不启用（避免编译依赖复杂度）

---

## 六、文件变更清单

```
# 新建 ────────────────────────────────────────
crates/websearch/                          # [新建目录]
├── Cargo.toml                             # [新建]
└── src/
    ├── lib.rs                             # [新建] 入口，pub use 所有 public API
    ├── error.rs                           # [新建] SearchError 枚举
    ├── types.rs                           # [新建] SearchResult, SearchConfig 等
    ├── searcher.rs                        # [新建] search() 降级链协调器
    ├── duckduckgo/
    │   ├── mod.rs                         # [新建]
    │   ├── lite.rs                        # [新建] lite.duckduckgo.com
    │   ├── html.rs                        # [新建] html.duckduckgo.com 增强版
    │   └── instant_answer.rs              # [新建] api.duckduckgo.com
    ├── searxng.rs                         # [新建] SearXNG 客户端
    ├── fetcher.rs                         # [新建] 网页抓取（替代 tarzi）
    ├── anti_detection/
    │   ├── mod.rs                         # [新建]
    │   ├── user_agent.rs                  # [新建] UA 池
    │   ├── rate_limiter.rs                # [新建] 速率限制器
    │   ├── cookie_mgr.rs                  # [新建] Cookie 管理
    │   └── proxy.rs                       # [新建] 代理管理器
    └── html_utils.rs                      # [新建] HTML 工具函数（迁移自 web_search.rs）

# 修改 ────────────────────────────────────────
./Cargo.toml                              # [修改] + workspace member, + workspace dep
crates/framework/Cargo.toml               # [修改] + rust-agent-websearch, - tarzi
crates/framework/src/tools/web_search.rs  # [修改] 精简为调用 rust_agent_websearch
crates/framework/src/tools/web_fetch.rs   # [修改] 精简为调用 rust_agent_websearch
```

---

## 七、关键技术决策

1. **独立 crate 而非 framework 子模块**——可被任意 Rust 项目独立使用，不绑定框架生态
2. **不依赖 `rust-agent-core`**——纯通用库，仅依赖 `tokio`、`reqwest`、`scraper`、`serde_json`
3. **移除 `tarzi` 依赖**——不需要外部浏览器，纯 Rust HTTP 实现
4. **`wreq` / BoringSSL 作为可选 feature**——避免默认编译依赖复杂度（cmake/perl/libclang）
5. **CAPTCHA 检测+告警 模式**——非交互环境下检测到 CAPTCHA 返回明确错误而非阻塞
6. **保持现有 `#[tool]` 工具接口不变**——框架层的 `WebSearch` / `WebFetch` 工具行为无变化

---

## 八、验证步骤

1. `cargo build -p rust-agent-websearch` 独立编译通过
2. `cargo test -p rust-agent-websearch` 单元测试通过（HTML fixture 解析测试）
3. `cargo test -p rust-agent-framework` 现有测试兼容（`test_web_search_basic`、`test_web_fetch_invalid_url`）
4. 集成测试：真实网络请求，验证降级链路（Lite → Instant Answer → HTML）
5. 反爬测试：连续 20 次请求不触发 DuckDuckGo CAPTCHA
6. 速率控制测试：验证请求间隔满足配置要求

---

## 九、风险与缓解

| 风险 | 缓解 |
|---|---|
| DuckDuckGo 改版导致 CSS 选择器失效 | 单元测试覆盖离线 HTML fixture；多后端降级 |
| DuckDuckGo 全面启用 Turnstile CAPTCHA | 降级到 Instant Answer API；切换到 SearXNG |
| 公共 SearXNG 实例不可用 | 支持自建实例配置；多实例自动切换 |
| 网络隔离环境不可达 DuckDuckGo | 代理支持；SearXNG 作为替代 |
| `wreq` 编译复杂度高 | 作为可选 feature，非默认启用 |

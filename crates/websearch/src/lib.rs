//! # rust-websearch
//!
//! 纯 Rust 实现、无需 API Key 的网络搜索库。
//!
//! ## 快速开始
//!
//! ```rust,no_run
//! use rust_websearch::{search, SearchConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = SearchConfig::default();
//! let results = search("rust programming", &config).await?;
//! println!("Found {} results from {:?}", results.results.len(), results.source);
//! # Ok(())
//! # }
//! ```
//!
//! ## 搜索后端（免 API Key）
//!
//! - **DuckDuckGo Lite** (`lite.duckduckgo.com`) — 首选，最轻量，最少反爬
//! - **DuckDuckGo Instant Answer** (`api.duckduckgo.com`) — JSON API，适合知识类查询
//! - **DuckDuckGo HTML** (`html.duckduckgo.com`) — 通用网页搜索
//! - **SearXNG** — 需自建实例，聚合 70+ 搜索引擎
//!
//! ## 网页抓取
//!
//! 基于 servo-fetch（内嵌 Servo 浏览器引擎）实现浏览器级网页渲染和内容提取：
//! - 真实 JS 执行（SpiderMonkey 引擎）
//! - 布局感知正文提取（自动去除导航/页脚/广告）
//! - Markdown 格式输出
//! - SPA 页面水合等待
//!
//! ## 反爬机制
//!
//! - User-Agent 池轮换
//! - 速率控制 + 随机抖动

pub mod anti_detection;
pub mod bing;
pub mod content_cleaner;
pub mod error;
pub mod fetcher;
pub mod html_utils;
pub mod probe;
pub mod scraper_fallback;
pub mod searcher;
pub mod searxng;
pub mod types;

pub mod duckduckgo;

// ── 重新导出核心公共 API ──

pub use anti_detection::{RateLimiter, random_user_agent};
pub use bing::search_bing;
pub use content_cleaner::{CleanMode, ContentCleaner, score_content};
pub use error::SearchError;
pub use fetcher::fetch_page;
pub use probe::{clear_cache as clear_probe_cache, BackendKind, Reachability};
pub use searcher::search;
pub use searxng::search_searxng;
pub use types::{FetchConfig, FetchedPage, SearchConfig, SearchResult, SearchResults, SearchSource};

//! # rust-agent-websearch
//!
//! `rust-websearch` 的 Agent Tool 封装，提供 `WebSearch` 和 `WebFetch` 两个 `#[tool]` 工具。
//!
//! ## 使用
//!
//! ```rust,no_run
//! use rust_agent_core::ToolRegistry;
//! use rust_agent_websearch::{WebSearch, WebFetch};
//!
//! let mut registry = ToolRegistry::new();
//! registry.register(WebSearch);
//! registry.register(WebFetch);
//! ```

pub mod context_provider;
pub mod web_search;
pub mod web_fetch;

pub use context_provider::WebSearchContextProvider;
pub use web_search::WebSearch;
pub use web_fetch::WebFetch;

use std::sync::OnceLock;
use rust_agent_core::ToolRegistry;

// ── 共享配置 ──

/// 由 `WebSearchContextProvider` 设置的共享配置，供 `web_search` 和 `web_fetch` 工具读取。
///
/// 优先级：共享配置 > 环境变量。
pub struct WebSearchSharedConfig {
    pub proxy_url: Option<String>,
    pub searxng_url: Option<String>,
    pub language: Option<String>,
}

static SHARED_CONFIG: OnceLock<WebSearchSharedConfig> = OnceLock::new();

/// 设置共享配置（通常由 `WebSearchContextProvider` 在注入工具前调用）。
/// 仅允许设置一次；后续调用将被忽略。
pub fn set_shared_config(config: WebSearchSharedConfig) {
    let _ = SHARED_CONFIG.set(config);
}

pub(crate) fn get_shared_config() -> Option<&'static WebSearchSharedConfig> {
    SHARED_CONFIG.get()
}

/// 注册所有 web 搜索工具。
pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(WebSearch);
    registry.register(WebFetch);
}

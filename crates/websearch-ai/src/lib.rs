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

pub mod web_search;
pub mod web_fetch;

pub use web_search::WebSearch;
pub use web_fetch::WebFetch;

use rust_agent_core::ToolRegistry;

/// 注册所有 web 搜索工具。
pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(WebSearch);
    registry.register(WebFetch);
}

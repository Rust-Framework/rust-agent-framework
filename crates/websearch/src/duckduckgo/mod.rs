//! DuckDuckGo 多后端搜索模块。
//!
//! 提供三个后端：
//! - `lite` — `lite.duckduckgo.com`（最轻量，最少反爬）
//! - `instant_answer` — `api.duckduckgo.com`（JSON API，知识类查询）
//! - `html` — `html.duckduckgo.com`（通用网页搜索）

pub use html::search_html;
pub use instant_answer::search_instant_answer;
pub use lite::search_lite;

pub mod html;
pub mod instant_answer;
pub mod lite;

//! 反爬检测模块入口。
//!
//! 提供构建反爬增强 HTTP 客户端的工具函数，
//! 以及 User-Agent 池、速率控制。

pub mod rate_limiter;
pub mod user_agent;

pub use rate_limiter::RateLimiter;
pub use user_agent::random_user_agent;

use crate::error::SearchError;
use crate::types::SearchConfig;

/// 构建一个配置好反爬增强的 reqwest Client。
///
/// 自动应用：
/// - 随机 User-Agent
/// - 超时配置
/// - 重定向策略（最多 5 次跳转）
/// - 代理（如果配置了 `proxy_url`）
pub fn build_client(config: &SearchConfig) -> Result<reqwest::Client, SearchError> {
    let ua = random_user_agent();
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .user_agent(ua)
        .redirect(reqwest::redirect::Policy::limited(5));

    // 代理
    if let Some(ref proxy_url) = config.proxy_url {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| {
            SearchError::Config(format!("Invalid proxy URL: {e}"))
        })?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|e| SearchError::Config(format!("Failed to build HTTP client: {e}")))
}

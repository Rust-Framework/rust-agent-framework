//! 反爬检测模块入口。
//!
//! 提供构建反爬增强 HTTP 客户端的工具函数，
//! 以及 User-Agent 池、速率控制、请求重试。

pub mod rate_limiter;
pub mod user_agent;

pub use rate_limiter::RateLimiter;
pub use user_agent::random_user_agent;

use crate::error::SearchError;
use crate::types::SearchConfig;
use tracing::debug;

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

/// 带重试的 HTTP 请求执行器。
///
/// 对瞬时网络错误（超时、连接重置）自动重试，最多 `max_retries` 次。
/// 每次重试前等待递增的退避时间（500ms, 1s, 2s...）。
/// 对业务错误（HTTP 4xx/5xx、CAPTCHA 等）不重试。
pub async fn retry_request<F, Fut, T>(
    label: &str,
    max_retries: u32,
    f: F,
) -> Result<T, SearchError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, SearchError>>,
{
    let mut last_err = None;
    for attempt in 0..=max_retries {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let is_retryable = matches!(
                    &e,
                    SearchError::Timeout(_)
                        | SearchError::Network(_)
                );
                if !is_retryable || attempt == max_retries {
                    last_err = Some(e);
                    break;
                }
                let delay = std::time::Duration::from_millis(500 * (1 << attempt));
                debug!(
                    "{label} attempt {}/{} failed (retryable), retrying in {:?}: {}",
                    attempt + 1,
                    max_retries + 1,
                    delay,
                    last_err.as_ref().unwrap_or(&e),
                );
                tokio::time::sleep(delay).await;
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or(SearchError::NoResults))
}

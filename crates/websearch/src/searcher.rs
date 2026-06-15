//! 搜索协调器 —— 多后端降级链。
//!
//! `search()` 函数按以下顺序尝试：
//! 1. DuckDuckGo Lite（首选，最稳定）
//! 2. DuckDuckGo Instant Answer（JSON API）
//! 3. DuckDuckGo HTML（通用搜索）
//! 4. SearXNG（如果配置了实例 URL）

use crate::anti_detection::RateLimiter;
use crate::duckduckgo;
use crate::error::SearchError;
use crate::types::{SearchConfig, SearchResults};
use std::sync::Arc;
use tracing::{debug, warn};

/// 全局速率限制器（跨所有搜索请求共享）。
fn global_rate_limiter() -> &'static Arc<RateLimiter> {
    static LIMITER: std::sync::OnceLock<Arc<RateLimiter>> = std::sync::OnceLock::new();
    LIMITER.get_or_init(|| Arc::new(RateLimiter::new()))
}

/// 执行搜索，按降级链依次尝试后端。
///
/// # 降级策略
///
/// 1. DuckDuckGo Lite — 首选，纯 HTTP，最少反爬
/// 2. DuckDuckGo Instant Answer — JSON API，适合知识类查询
/// 3. DuckDuckGo HTML — 通用网页搜索（可能 CAPTCHA）
/// 4. SearXNG — 需要配置 `config.searxng_url`
pub async fn search(query: &str, config: &SearchConfig) -> Result<SearchResults, SearchError> {
    if query.trim().is_empty() {
        return Err(SearchError::Config("Search query cannot be empty".into()));
    }

    // 速率控制
    global_rate_limiter().wait(config.min_interval_ms).await;

    // 1. 尝试 DuckDuckGo Lite
    match duckduckgo::search_lite(query, config).await {
        Ok(results) => {
            debug!("DuckDuckGo Lite succeeded: {} results", results.results.len());
            return Ok(results);
        }
        Err(e) => {
            warn!("DuckDuckGo Lite failed: {e}");
        }
    }

    // 2. 尝试 Instant Answer
    match duckduckgo::search_instant_answer(query, config).await {
        Ok(results) => {
            debug!(
                "DuckDuckGo Instant Answer succeeded: {} results",
                results.results.len()
            );
            return Ok(results);
        }
        Err(e) => {
            warn!("DuckDuckGo Instant Answer failed: {e}");
        }
    }

    // 3. 尝试 HTML
    match duckduckgo::search_html(query, config).await {
        Ok(results) => {
            debug!("DuckDuckGo HTML succeeded: {} results", results.results.len());
            return Ok(results);
        }
        Err(e) => {
            warn!("DuckDuckGo HTML failed: {e}");
        }
    }

    // 4. 尝试 SearXNG（如果配置了）
    if config.searxng_url.is_some() {
        match crate::searxng::search_searxng(query, config).await {
            Ok(results) => {
                debug!("SearXNG succeeded: {} results", results.results.len());
                return Ok(results);
            }
            Err(e) => {
                warn!("SearXNG failed: {e}");
            }
        }
    }

    Err(SearchError::NoResults)
}

//! 网页内容抓取。
//!
//! 基于 servo-fetch（内嵌 Servo 浏览器引擎）实现浏览器级网页渲染和内容提取。
//! 支持 JavaScript 执行、布局感知正文提取、SPA 页面水合等待。

use crate::anti_detection::RateLimiter;
use crate::error::SearchError;
use crate::types::{FetchConfig, FetchedPage};
use std::sync::Arc;
use std::time::Duration;

fn fetch_rate_limiter() -> &'static Arc<RateLimiter> {
    static LIMITER: std::sync::OnceLock<Arc<RateLimiter>> = std::sync::OnceLock::new();
    LIMITER.get_or_init(|| Arc::new(RateLimiter::new()))
}

/// 抓取网页内容。
///
/// ## 处理流程
///
/// 1. 速率控制
/// 2. 使用 servo-fetch 的 Servo 引擎渲染页面（含 JS 执行）
/// 3. 提取可读 Markdown 内容（布局感知，自动去除导航/页脚/广告）
/// 4. 内容截断保护
pub async fn fetch_page(url: &str, config: &FetchConfig) -> Result<FetchedPage, SearchError> {
    if url.is_empty() {
        return Err(SearchError::Config("URL cannot be empty".into()));
    }

    // 速率控制
    fetch_rate_limiter().wait(config.min_interval_ms).await;

    // 构建 servo-fetch FetchOptions
    let mut opts = servo_fetch::FetchOptions::new(url)
        .timeout(Duration::from_secs(config.timeout_secs));

    // SPA 水合等待
    if config.settle_ms > 0 {
        opts = opts.settle(Duration::from_millis(config.settle_ms));
    }

    // User-Agent
    let ua = config
        .user_agent
        .as_deref()
        .unwrap_or_else(|| crate::anti_detection::random_user_agent());
    opts = opts.user_agent(ua);

    tracing::debug!(
        url = %url,
        timeout_secs = config.timeout_secs,
        settle_ms = config.settle_ms,
        "Fetching page via servo-fetch"
    );

    // 执行 servo-fetch（async API）
    let page = servo_fetch::fetch(&opts).await?;

    // 提取内容：优先 Markdown，降级纯文本
    let content = page
        .markdown()
        .unwrap_or_else(|_| page.inner_text.clone());

    let title = page.title.clone().unwrap_or_default();

    tracing::debug!(
        url = %url,
        title = %title,
        content_len = content.len(),
        "Page fetched and extracted"
    );

    // 截断处理
    let content_length = content.len();
    let (content, truncated) = if content_length > config.max_content_bytes {
        let truncate_at = content
            .char_indices()
            .take(config.max_content_bytes)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        let truncated_content = &content[..truncate_at];
        let truncated = format!(
            "{truncated_content}\n\n[Content truncated: {total} bytes total, showing first {shown} bytes. Use a smaller scope or more specific query to get relevant data.]",
            total = content_length,
            shown = truncate_at
        );
        (truncated, true)
    } else {
        (content, false)
    };

    Ok(FetchedPage {
        url: url.to_string(),
        final_url: url.to_string(),
        title,
        content,
        content_length,
        truncated,
        status_code: 200,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_config_default() {
        let config = FetchConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_content_bytes, 50 * 1024);
        assert!(config.user_agent.is_none());
        assert_eq!(config.settle_ms, 0);
    }
}

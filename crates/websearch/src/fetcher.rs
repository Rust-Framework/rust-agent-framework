//! 网页内容抓取。
//!
//! 基于 servo-fetch（内嵌 Servo 浏览器引擎）实现浏览器级网页渲染和内容提取。
//! 支持 JavaScript 执行、布局感知正文提取、SPA 页面水合等待。
//!
//! ## 内容提取管线
//!
//! 1. servo-fetch 渲染并提取 Markdown
//! 2. ContentCleaner 后处理清洗（去噪、页脚检测）
//! 3. 质量评分——若不达标，回退到 scraper 提取
//! 4. 截断保护

use crate::anti_detection::RateLimiter;
use crate::content_cleaner::{ContentCleaner, score_content};
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
/// 4. ContentCleaner 后处理清洗
/// 5. 质量评分——若不达标且回退启用，使用 scraper 重试
/// 6. 内容截断保护
pub async fn fetch_page(url: &str, config: &FetchConfig) -> Result<FetchedPage, SearchError> {
    if url.is_empty() {
        return Err(SearchError::Config("URL cannot be empty".into()));
    }

    // 速率控制
    fetch_rate_limiter().wait(config.min_interval_ms).await;

    let cleaner = ContentCleaner::new(config.clean_mode);

    // 尝试 servo-fetch
    let (title, raw_content, final_url) = match try_servo_fetch(url, config).await {
        Ok((t, c, fu)) => (t, c, fu),
        Err(e) => {
            tracing::warn!(url = %url, error = %e, "servo-fetch failed, attempting scraper fallback");

            if config.fallback_enabled {
                return crate::scraper_fallback::extract_with_scraper(url, config).await;
            }
            return Err(e);
        }
    };

    // 后处理清洗
    let cleaned = cleaner.clean(&raw_content);

    tracing::debug!(
        url = %url,
        raw_len = raw_content.len(),
        cleaned_len = cleaned.len(),
        "Content cleaned"
    );

    // 质量评分
    let quality = score_content(&cleaned);
    tracing::debug!(url = %url, quality = quality, threshold = config.quality_threshold, "Content quality scored");

    // 决定最终使用的 content
    let (content, source) = if quality < config.quality_threshold && config.fallback_enabled {
        tracing::info!(
            url = %url,
            quality = quality,
            threshold = config.quality_threshold,
            "Quality below threshold, attempting scraper fallback"
        );

        match crate::scraper_fallback::extract_with_scraper(url, config).await {
            Ok(fallback_page) => {
                let fallback_cleaned = cleaner.clean(&fallback_page.content);
                let fallback_quality = score_content(&fallback_cleaned);

                if fallback_quality > quality {
                    tracing::info!(
                        url = %url,
                        fallback_quality = fallback_quality,
                        original_quality = quality,
                        "Scraper fallback produced better content"
                    );
                    (fallback_cleaned, "scraper-fallback")
                } else {
                    tracing::info!(
                        url = %url,
                        "Scraper fallback did not improve quality, keeping servo-fetch output"
                    );
                    (cleaned, "servo-fetch")
                }
            }
            Err(e) => {
                tracing::warn!(url = %url, error = %e, "Scraper fallback failed, keeping servo-fetch output");
                (cleaned, "servo-fetch")
            }
        }
    } else {
        (cleaned, "servo-fetch")
    };

    tracing::info!(
        url = %url,
        title = %title,
        content_len = content.len(),
        quality = quality,
        source = source,
        "Page fetched successfully"
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
        final_url,
        title,
        content,
        content_length,
        truncated,
        status_code: 200,
    })
}

/// 使用 servo-fetch 抓取页面并返回标题、原始内容和最终 URL。
async fn try_servo_fetch(
    url: &str,
    config: &FetchConfig,
) -> Result<(String, String, String), SearchError> {
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

    let page = servo_fetch::fetch(&opts).await?;

    let content = page
        .markdown()
        .unwrap_or_else(|_| page.inner_text.clone());

    let title = page.title.clone().unwrap_or_default();
    let final_url = url.to_string(); // servo-fetch 目前不暴露 final URL

    Ok((title, content, final_url))
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
        assert!(config.fallback_enabled);
        assert_eq!(config.quality_threshold, 0.4);
    }
}

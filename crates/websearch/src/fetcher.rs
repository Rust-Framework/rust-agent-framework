//! 网页内容抓取（替代 tarzi::WebFetcher）。
//!
//! 纯 reqwest HTTP 实现，不依赖外部浏览器。
//! 提取 HTML title 和正文，转换为纯文本。

use crate::anti_detection::RateLimiter;
use crate::error::SearchError;
use crate::types::{FetchedPage, FetchConfig};
use scraper::{Html, Selector};
use std::sync::Arc;

fn fetch_rate_limiter() -> &'static Arc<RateLimiter> {
    static LIMITER: std::sync::OnceLock<Arc<RateLimiter>> = std::sync::OnceLock::new();
    LIMITER.get_or_init(|| Arc::new(RateLimiter::new()))
}

/// 抓取网页内容。
///
/// 使用纯 HTTP 请求获取页面，提取标题和正文，转为纯文本。
pub async fn fetch_page(url: &str, config: &FetchConfig) -> Result<FetchedPage, SearchError> {
    if url.is_empty() {
        return Err(SearchError::Config("URL cannot be empty".into()));
    }

    fetch_rate_limiter().wait(config.min_interval_ms).await;

    let ua = crate::anti_detection::random_user_agent();
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .user_agent(ua)
        .redirect(reqwest::redirect::Policy::limited(5));

    if let Some(ref proxy_url) = config.proxy_url {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| SearchError::Config(format!("Invalid proxy URL: {e}")))?;
        builder = builder.proxy(proxy);
    }

    let client = builder
        .build()
        .map_err(|e| SearchError::Config(format!("Failed to build HTTP client: {e}")))?;

    let response = client.get(url).send().await?;

    let status_code = response.status().as_u16();
    let final_url = response.url().to_string();

    if !response.status().is_success() {
        return Err(SearchError::HttpStatus {
            code: status_code,
            message: format!("HTTP {status_code} for {final_url}"),
        });
    }

    // 检查 Content-Type，只处理 HTML 和文本
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_default();

    // 判断是否为 HTML
    let is_html = content_type.contains("text/html") || content_type.is_empty();

    let html = response.text().await.map_err(|e| {
        SearchError::Parse(format!("Failed to read response body: {e}"))
    })?;

    let (title, content) = if is_html {
        extract_text_content(&html)
    } else {
        // 非 HTML 内容，直接作为文本返回
        (String::new(), html)
    };

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
            "{truncated_content}\n\n[truncated — content too large]"
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
        status_code,
    })
}

/// 从 HTML 中提取标题和正文（纯文本）。
fn extract_text_content(html: &str) -> (String, String) {
    let document = Html::parse_document(html);

    // 提取 title
    let title = document
        .select(&Selector::parse("title").unwrap())
        .next()
        .map(|t| t.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .unwrap_or_default();

    // 提取正文：去除 script/style 后获取 body 文本
    let body_text = document
        .select(&Selector::parse("body").unwrap())
        .next()
        .map(|body| {
            // 先去除 script 和 style
            let mut html_str = body.inner_html();
            html_str = remove_tags(&html_str, "script");
            html_str = remove_tags(&html_str, "style");
            html_str = remove_tags(&html_str, "noscript");

            // 解码 HTML 实体并去除标签
            crate::html_utils::clean_html(&html_str)
        })
        .unwrap_or_default();

    // fallback: 如果没有 body，使用全部文本
    let content = if body_text.trim().is_empty() {
        let mut html_str = html.to_string();
        html_str = remove_tags(&html_str, "script");
        html_str = remove_tags(&html_str, "style");
        crate::html_utils::clean_html(&html_str)
    } else {
        body_text
    };

    (title, content.trim().to_string())
}

/// 从 HTML 字符串中移除指定标签（包括内容）。
fn remove_tags(html: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");

    let mut result = String::with_capacity(html.len());
    let mut rest = html;

    loop {
        let Some(start_idx) = rest.find(&open) else {
            result.push_str(rest);
            break;
        };

        result.push_str(&rest[..start_idx]);

        // 找到标签闭合的 >
        let after_open = &rest[start_idx..];
        let Some(end_of_open) = after_open.find('>') else {
            result.push_str(&rest[start_idx..]);
            break;
        };

        let after_tag = &after_open[end_of_open + 1..];

        // 找到对应的 </tag>
        let Some(close_idx) = after_tag.find(&close) else {
            result.push_str(&rest[start_idx..]);
            break;
        };

        rest = &after_tag[close_idx + close.len()..];
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_content() {
        let html = r#"<!DOCTYPE html>
<html>
<head><title>Test Page</title></head>
<body>
    <h1>Hello World</h1>
    <p>This is a <b>test</b> paragraph.</p>
    <script>console.log('should be removed');</script>
    <style>body { color: red; }</style>
</body>
</html>"#;

        let (title, content) = extract_text_content(html);
        assert_eq!(title, "Test Page");
        assert!(content.contains("Hello World"));
        assert!(content.contains("This is a test paragraph"));
        assert!(!content.contains("console.log"));
        assert!(!content.contains("color: red"));
    }

    #[test]
    fn test_remove_tags() {
        let html = "<div>keep me</div><script>remove me</script><p>keep too</p>";
        let result = remove_tags(html, "script");
        assert!(result.contains("keep me"));
        assert!(!result.contains("remove me"));
        assert!(result.contains("keep too"));
    }

    #[test]
    fn test_extract_text_no_body() {
        let html = "<html>Hello <b>World</b></html>";
        let (title, content) = extract_text_content(html);
        assert!(content.contains("Hello World"));
    }
}

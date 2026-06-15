//! 网页内容抓取。
//!
//! 主动态 HTTP 请求抓取 + 字符编码自动检测 + 正文智能提取。
//! 支持中文站点 GBK/GB2312/GB18030/Big5 等编码的自动识别。

use crate::anti_detection::RateLimiter;
use crate::content_extractor::extract_main_content;
use crate::encoding::{decode_bytes, parse_content_type_charset, parse_meta_charset};
use crate::error::SearchError;
use crate::types::{FetchedPage, FetchConfig};
use std::sync::Arc;

fn fetch_rate_limiter() -> &'static Arc<RateLimiter> {
    static LIMITER: std::sync::OnceLock<Arc<RateLimiter>> = std::sync::OnceLock::new();
    LIMITER.get_or_init(|| Arc::new(RateLimiter::new()))
}

/// 抓取网页内容。
///
/// ## 处理流程
///
/// 1. 发送 HTTP GET 请求，获取响应字节（不直接用 `.text()`，以便编码检测）
/// 2. 从 Content-Type header 和 HTML <meta> 标签提取声明的 charset
/// 3. 使用 `encoding_rs` 自动检测并解码为 UTF-8
/// 4. 对 HTML 页面使用正文提取算法，去噪后返回纯文本
/// 5. 对非 HTML 内容直接作为文本返回
pub async fn fetch_page(url: &str, config: &FetchConfig) -> Result<FetchedPage, SearchError> {
    if url.is_empty() {
        return Err(SearchError::Config("URL cannot be empty".into()));
    }

    fetch_rate_limiter().wait(config.min_interval_ms).await;

    let client = build_fetch_client(config)?;

    let response = client.get(url).send().await?;

    let status_code = response.status().as_u16();
    let final_url = response.url().to_string();

    if !response.status().is_success() {
        return Err(SearchError::HttpStatus {
            code: status_code,
            message: format!("HTTP {status_code} for {final_url}"),
        });
    }

    // ── 编码检测 ──
    // 1. 从 Content-Type header 获取声明的 charset
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let declared_charset = parse_content_type_charset(content_type);

    // 判断是否为 HTML
    let is_html = content_type.contains("text/html")
        || content_type.is_empty()
        || content_type.contains("application/xhtml");

    // ── 获取原始字节 ──
    let bytes = response.bytes().await.map_err(|e| {
        SearchError::Parse(format!("Failed to read response body: {e}"))
    })?;

    // 2. 从 HTML meta 标签提取 charset（如果声明的不够确定）
    let meta_charset = if is_html {
        parse_meta_charset(&bytes)
    } else {
        None
    };

    // 选择最终的 charset 提示
    let charset_hint = meta_charset
        .or(declared_charset)
        .or_else(|| {
            // 根据 final_url 域名猜测编码（中国域名大概率是 GBK/UTF-8）
            guess_charset_by_domain(&final_url)
        });

    // 解码为 UTF-8
    let html = decode_bytes(&bytes, charset_hint.as_deref());

    tracing::debug!(
        url = %url,
        final_url = %final_url,
        is_html = is_html,
        charset = ?charset_hint,
        bytes_len = bytes.len(),
        decoded_len = html.len(),
        "Page fetched and decoded"
    );

    // ── 内容提取 ──
    let (title, content) = if is_html {
        let extracted = extract_main_content(&html);

        // 解析标题
        let title = extracted
            .strip_prefix("Title: ")
            .and_then(|s| s.split("\n\n").next())
            .unwrap_or("")
            .to_string();

        let body = if extracted.starts_with("Title: ") {
            // 移除标题行
            let after_title = extracted
                .splitn(2, "\n\n")
                .nth(1)
                .unwrap_or(&extracted);
            after_title.to_string()
        } else {
            extracted
        };

        (title, body)
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
        status_code,
    })
}

/// 构建抓取专用 HTTP 客户端。
fn build_fetch_client(config: &FetchConfig) -> Result<reqwest::Client, SearchError> {
    let ua = crate::anti_detection::random_user_agent();
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs))
        .user_agent(ua)
        .redirect(reqwest::redirect::Policy::limited(5))
        // 设置中文友好的默认请求头
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_static(
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
                ),
            );
            headers.insert(
                reqwest::header::ACCEPT_LANGUAGE,
                reqwest::header::HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
            );
            headers
        });

    if let Some(ref proxy_url) = config.proxy_url {
        let proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|e| SearchError::Config(format!("Invalid proxy URL: {e}")))?;
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| SearchError::Config(format!("Failed to build HTTP client: {e}")))
}

/// 根据域名猜测可能的编码。
///
/// `.cn` 域名大概率使用 GBK/UTF-8，`.jp` 使用 Shift_JIS 等。
fn guess_charset_by_domain(url: &str) -> Option<String> {
    let domain = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .to_lowercase();

    if domain.ends_with(".cn") || domain.ends_with(".com.cn") {
        // 中国域名，优先尝试 GBK 再 UTF-8（实际解码时 auto-detection 会处理）
        Some("gbk".to_string())
    } else if domain.ends_with(".jp") {
        Some("shift_jis".to_string())
    } else if domain.ends_with(".tw") || domain.ends_with(".hk") {
        Some("big5".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guess_charset_cn() {
        assert_eq!(
            guess_charset_by_domain("https://finance.sina.com.cn/page"),
            Some("gbk".to_string())
        );
    }

    #[test]
    fn test_guess_charset_non_cn() {
        assert_eq!(
            guess_charset_by_domain("https://example.com/page"),
            None
        );
    }

    #[test]
    fn test_build_fetch_client() {
        let config = FetchConfig::default();
        let client = build_fetch_client(&config);
        assert!(client.is_ok());
    }
}

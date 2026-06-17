//! 基于 scraper 的内容提取回退策略。
//!
//! 当 servo-fetch 提取结果质量不满足阈值时，使用 reqwest + scraper
//! 通过 CSS 选择器优先提取正文区域。支持通用选择器链、站点专属选择器
//! 以及最大文本块启发式回退。

use crate::error::SearchError;
use crate::types::{FetchedPage, FetchConfig};
use scraper::{Html, Selector};
use url::Url;

// ── 站点专属选择器 ──

/// 按域名返回专属的 CSS 选择器列表（按优先级排序）。
fn domain_specific_selectors(host: &str) -> Vec<&'static str> {
    let host_lower = host.to_lowercase();

    if host_lower.contains("wikipedia.org") {
        return vec![
            "#mw-content-text .mw-parser-output",
            "#bodyContent",
            ".mw-parser-output",
        ];
    }
    if host_lower.contains("github.com") {
        return vec![
            ".markdown-body",
            "[data-hpc] article",
            ".blob-wrapper",
        ];
    }
    if host_lower.contains("stackoverflow.com") || host_lower.contains("stackexchange.com") {
        return vec![
            ".s-prose",
            ".answer .js-post-body",
            ".question .js-post-body",
            "#mainbar",
        ];
    }
    if host_lower.contains("reddit.com") {
        return vec![
            "[slot=\"text-body\"]",
            ".post-content",
            "[data-testid=\"post-content\"]",
            "shreddit-post",
        ];
    }
    if host_lower.contains("medium.com") {
        return vec!["article section", "article .section-content"];
    }
    if host_lower.contains("developer.mozilla.org") || host_lower.contains("mdn.") {
        return vec![".main-page-content", ".article-content"];
    }
    if host_lower.contains("docs.rs") {
        return vec![".main-content .docblock", ".main-content"];
    }
    if host_lower.contains("docs.python.org") {
        return vec!["div.body section", "div.document"];
    }
    if host_lower.contains("zhihu.com") {
        return vec![".RichContent-inner", ".Post-RichText", ".AnswerCard .RichText"];
    }
    if host_lower.contains("juejin.cn") {
        return vec![".article-content", ".markdown-body"];
    }
    if host_lower.contains("csdn.net") {
        return vec!["#article_content", "#content_views", "article"];
    }
    if host_lower.contains("blog.csdn.net") {
        return vec!["#article_content", "#content_views", "article"];
    }
    if host_lower.contains("jianshu.com") {
        return vec![".show-content-free", "article ._2rhmJa"];
    }
    if host_lower.contains("segmentfault.com") {
        return vec![".article-content", ".article-body"];
    }

    vec![]
}

/// 通用主流内容选择器（按优先级排序）。
fn generic_content_selectors() -> Vec<&'static str> {
    vec![
        "article",
        "main",
        "[role=\"main\"]",
        ".post-content",
        ".article-content",
        ".article-body",
        ".entry-content",
        ".content-body",
        ".post-body",
        "#content",
        "#main-content",
        ".main-content",
        ".page-content",
        ".container main",
        "#main",
    ]
}

/// 需要移除的元素选择器（导航、侧边栏、页脚等）。
#[allow(dead_code)]
fn elements_to_remove() -> Vec<&'static str> {
    vec![
        "nav",
        "header",
        "footer",
        "aside",
        ".sidebar",
        ".nav",
        ".navbar",
        ".navigation",
        ".menu",
        ".footer",
        ".header",
        ".comments",
        "#comments",
        ".related-posts",
        ".recommended",
        ".advertisement",
        ".ad",
        ".ads",
        ".social-share",
        ".share-buttons",
        ".cookie-banner",
        ".cookie-notice",
        ".newsletter-signup",
        "script",
        "style",
        "noscript",
        "iframe",
        ".toc",
        "#toc",
        ".table-of-contents",
    ]
}

// ── 主要提取逻辑 ──

/// 使用 scraper 回退策略抓取网页内容。
///
/// ## 策略
///
/// 1. 站点专属选择器优先
/// 2. 通用语义选择器
/// 3. 最大文本块启发式
pub async fn extract_with_scraper(
    url_str: &str,
    config: &FetchConfig,
) -> Result<FetchedPage, SearchError> {
    let client = build_reqwest_client(config)?;

    let response = client.get(url_str).send().await.map_err(|e| {
        SearchError::Network(format!("scraper fallback request failed: {e}"))
    })?;

    let final_url = response.url().to_string();

    if !response.status().is_success() {
        return Err(SearchError::Network(format!(
            "scraper fallback returned {}",
            response.status()
        )));
    }

    let html = response.text().await.map_err(|e| {
        SearchError::Parse(format!("scraper fallback body read failed: {e}"))
    })?;

    let document = Html::parse_document(&html);

    // 提取标题
    let title = extract_title(&document);

    // 确定主机名用于站点专属选择器
    let host = Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_default();

    // 尝试提取内容
    let content = try_extract_content(&document, &host, config)
        .or_else(|| largest_text_block(&document));

    let content = content.unwrap_or_else(|| {
        "Content extraction failed: no suitable content block found.".to_string()
    });

    let content_length = content.len();
    let truncated = content_length > config.max_content_bytes;

    Ok(FetchedPage {
        url: url_str.to_string(),
        final_url,
        title,
        content,
        content_length,
        truncated,
        status_code: 200,
    })
}

/// 构建 reqwest 客户端。
fn build_reqwest_client(config: &FetchConfig) -> Result<reqwest::Client, SearchError> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.timeout_secs));

    if let Some(ref ua) = config.user_agent {
        builder = builder.user_agent(ua.clone());
    } else {
        builder = builder.user_agent(crate::anti_detection::random_user_agent());
    }

    if let Some(ref proxy) = config.proxy_url {
        let proxy = reqwest::Proxy::all(proxy).map_err(|e| {
            SearchError::Config(format!("invalid proxy URL: {e}"))
        })?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|e| SearchError::Config(format!("failed to build client: {e}")))
}

/// 从文档中提取标题。
fn extract_title(document: &Html) -> String {
    if let Ok(sel) = Selector::parse("title") {
        if let Some(el) = document.select(&sel).next() {
            let inner = el.text().collect::<Vec<_>>().join(" ");
            if !inner.trim().is_empty() {
                return inner.trim().to_string();
            }
        }
    }

    if let Ok(sel) = Selector::parse("h1") {
        if let Some(el) = document.select(&sel).next() {
            let inner = el.text().collect::<Vec<_>>().join(" ");
            if !inner.trim().is_empty() {
                return inner.trim().to_string();
            }
        }
    }

    String::new()
}

/// 尝试通过选择器提取内容。返回提取到的文本或 None。
fn try_extract_content(document: &Html, host: &str, _config: &FetchConfig) -> Option<String> {
    // 收集所有要尝试的选择器（站点专属 + 通用 + 用户自定义）
    let mut all_selectors: Vec<String> = Vec::new();

    // 站点专属选择器优先
    for sel in domain_specific_selectors(host) {
        all_selectors.push(sel.to_string());
    }

    // 用户自定义选择器
    if let Some(ref custom) = _config.domain_selectors {
        for sel in custom.values() {
            all_selectors.push(sel.clone());
        }
    }

    // 通用选择器
    for sel in generic_content_selectors() {
        all_selectors.push(sel.to_string());
    }

    // 按优先级尝试每个选择器
    for selector_str in &all_selectors {
        let Ok(selector) = Selector::parse(selector_str) else {
            continue;
        };

        if let Some(el) = document.select(&selector).next() {
            let inner_html = el.inner_html();
            if inner_html.len() < 100 {
                // 内容太少，可能不是主内容区
                continue;
            }

            // 解析为独立文档以便移除噪音元素
            let fragment = Html::parse_fragment(&inner_html);

            // 移除噪音（导航、侧边栏等）
            let cleaned_html = remove_noise_elements(&fragment, &inner_html);

            // 转为纯文本
            let text = crate::html_utils::clean_html(&cleaned_html);
            let trimmed = text.trim().to_string();

            // 检查内容是否足够
            if trimmed.chars().count() > 50 {
                return Some(trimmed);
            }
        }
    }

    None
}

/// 从 HTML 片段中移除噪音元素。
fn remove_noise_elements(_fragment: &Html, fallback_html: &str) -> String {
    // 尝试解析为完整文档以支持移除操作
    let wrapped = format!("<html><body>{}</body></html>", fallback_html);
    let _doc = Html::parse_document(&wrapped);

    // scraper 的 Html 是不可变的，所以我们不能真的移除元素
    // 替代方案：用 CSS 选择器信息指导文本提取

    // 简单策略：直接返回原始 HTML，依赖 clean_html 的 tag stripping
    // 更复杂的实现需要可变的 DOM 操作（可用 ego-tree 等）
    fallback_html.to_string()
}

/// 最大文本块启发式——找到 DOM 树中文本-标签比最高的节点。
fn largest_text_block(document: &Html) -> Option<String> {
    // 尝试 body 下的所有直接子元素
    let Ok(body_sel) = Selector::parse("body *") else {
        return None;
    };

    let mut best_text = String::new();
    let mut best_score = 0.0_f64;

    for el in document.select(&body_sel) {
        let html = el.inner_html();
        let text_content: String = el.text().collect::<Vec<_>>().join(" ");
        let text_chars = text_content.chars().count();
        let html_chars = html.chars().count();

        if text_chars < 100 || html_chars < 1 {
            continue;
        }

        // 文本-标签比（简化版，使用字符数比）
        let ratio = text_chars as f64 / html_chars as f64;

        // 额外加分：包含段落标签
        let p_count = html.matches("<p").count();
        let bonus = (p_count as f64 * 0.02).min(0.2);

        let score = ratio + bonus;

        if score > best_score && text_chars > best_text.chars().count() {
            best_score = score;
            best_text = crate::html_utils::clean_html(&html);
        }
    }

    if best_text.trim().is_empty() {
        None
    } else {
        Some(best_text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_selectors_wikipedia() {
        let selectors = domain_specific_selectors("en.wikipedia.org");
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"#mw-content-text .mw-parser-output"));
    }

    #[test]
    fn test_domain_selectors_github() {
        let selectors = domain_specific_selectors("github.com");
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&".markdown-body"));
    }

    #[test]
    fn test_domain_selectors_unknown() {
        let selectors = domain_specific_selectors("example.com");
        assert!(selectors.is_empty());
    }

    #[test]
    fn test_generic_selectors_not_empty() {
        let selectors = generic_content_selectors();
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"article"));
        assert!(selectors.contains(&"main"));
    }

    #[test]
    fn test_domain_selectors_csdn() {
        let selectors = domain_specific_selectors("blog.csdn.net");
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&"#article_content"));
    }

    #[test]
    fn test_domain_selectors_zhihu() {
        let selectors = domain_specific_selectors("zhihu.com");
        assert!(!selectors.is_empty());
        assert!(selectors.contains(&".RichContent-inner"));
    }
}

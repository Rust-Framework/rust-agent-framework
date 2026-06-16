//! DuckDuckGo HTML 后端（`html.duckduckgo.com`）。
//!
//! 通用网页搜索，使用 `scraper` crate 进行 CSS 选择器解析，
//! 替代原先的正则表达式解析方式。

use crate::error::SearchError;
use crate::html_utils::clean_html;
use crate::types::{SearchConfig, SearchResult, SearchResults, SearchSource};
use scraper::{Html, Selector};

const HTML_URL: &str = "https://html.duckduckgo.com/html/";

/// 通过 DuckDuckGo HTML 搜索。
pub async fn search_html(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    crate::anti_detection::retry_request("DuckDuckGo HTML", 1, || async {
        search_html_inner(query, config).await
    }).await
}

async fn search_html_inner(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    let client = crate::anti_detection::build_client(config)?;

    let response = client
        .get(HTML_URL)
        .query(&[("q", query)])
        .send()
        .await?;

    let status = response.status();

    // 检测 CAPTCHA（HTTP 202）
    if status == reqwest::StatusCode::ACCEPTED {
        return Err(SearchError::Captcha(
            "DuckDuckGo returned 202 — CAPTCHA challenge detected. Try using a proxy or SearXNG.".into(),
        ));
    }

    if status != reqwest::StatusCode::OK {
        return Err(SearchError::HttpStatus {
            code: status.as_u16(),
            message: format!("DuckDuckGo HTML returned {status}"),
        });
    }

    let html = response.text().await.map_err(|e| {
        SearchError::Parse(format!("Failed to read DuckDuckGo HTML response: {e}"))
    })?;

    // 检测隐藏在页面中的 CAPTCHA
    if html.contains("challenge-form") || html.contains("g-recaptcha") {
        return Err(SearchError::Captcha(
            "DuckDuckGo CAPTCHA detected in response body.".into(),
        ));
    }

    parse_html_results(&html, query, config.max_results)
}

fn parse_html_results(
    html: &str,
    query: &str,
    max_results: usize,
) -> Result<SearchResults, SearchError> {
    let document = Html::parse_document(html);

    // CSS 选择器（DuckDuckGo HTML 页面结构）：
    // .result__a    → 标题 + URL（<a class="result__a" href="...">标题</a>）
    // .result__snippet → 摘要（<a class="result__snippet">摘要文本</a>）

    let result_a_selector =
        Selector::parse("a.result__a").map_err(|e| SearchError::Parse(e.to_string()))?;
    let snippet_selector =
        Selector::parse("a.result__snippet").map_err(|e| SearchError::Parse(e.to_string()))?;

    let mut results: Vec<SearchResult> = Vec::new();
    let mut snippet_iter = document.select(&snippet_selector).peekable();

    for link_elem in document.select(&result_a_selector) {
        if results.len() >= max_results {
            break;
        }

        let raw_url = link_elem
            .value()
            .attr("href")
            .unwrap_or("")
            .to_string();

        let raw_title = link_elem.text().collect::<Vec<_>>().join(" ");

        let snippet = snippet_iter
            .next()
            .map(|s| s.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();

        let url = crate::html_utils::resolve_duckduckgo_url(&raw_url);
        let title = clean_html(&raw_title);
        let snippet = clean_html(&snippet);

        if url.is_empty() || url == "https:" {
            continue;
        }

        // 去重
        if results.iter().any(|r| r.url == url) {
            continue;
        }

        results.push(SearchResult {
            title: if title.is_empty() { url.clone() } else { title },
            url,
            snippet,
            source: SearchSource::DuckDuckGoHtml,
            rank: results.len() + 1,
        });
    }

    if results.is_empty() {
        return Err(SearchError::NoResults);
    }

    Ok(SearchResults {
        query: query.to_string(),
        results,
        source: SearchSource::DuckDuckGoHtml,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_html_results() {
        let html = r#"<!DOCTYPE html>
<html>
<body>
<div class="results">
    <div class="result">
        <a class="result__a" href="https://example.com">Example Title</a>
        <a class="result__snippet">This is a snippet about example.</a>
    </div>
    <div class="result">
        <a class="result__a" href="//rust-lang.org">Rust Programming Language</a>
        <a class="result__snippet">A language empowering everyone.</a>
    </div>
    <div class="result">
        <a class="result__a" href="/l/?uddg=https%3A%2F%2Fredirected%2Ecom">Redirected Site</a>
        <a class="result__snippet">This link uses DuckDuckGo redirect.</a>
    </div>
</div>
</body>
</html>"#;

        let results = parse_html_results(html, "test", 10).unwrap();
        assert_eq!(results.results.len(), 3);
        assert_eq!(results.results[0].title, "Example Title");
        assert_eq!(results.results[0].url, "https://example.com");
        assert_eq!(results.results[0].snippet, "This is a snippet about example.");
        assert_eq!(results.results[1].url, "https://rust-lang.org");
        // 重定向 URL 解析
        assert_eq!(results.results[2].url, "https://redirected.com");
    }

    #[test]
    fn test_parse_html_empty() {
        let html = "<html><body>No results found</body></html>";
        let result = parse_html_results(html, "test", 10);
        assert!(result.is_err());
    }
}

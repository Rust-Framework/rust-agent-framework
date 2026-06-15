//! DuckDuckGo Lite 后端（`lite.duckduckgo.com`）。
//!
//! Lite 版本是最轻量的 DuckDuckGo 接口，返回极简 HTML，
//! 不含 JavaScript，最不容易触发反爬机制。

use crate::error::SearchError;
use crate::html_utils::clean_html;
use crate::types::{SearchConfig, SearchResult, SearchResults, SearchSource};
use scraper::{Html, Selector};

const LITE_URL: &str = "https://lite.duckduckgo.com/lite/";

/// 通过 DuckDuckGo Lite 进行搜索。
pub async fn search_lite(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    let client = crate::anti_detection::build_client(config)?;
    let form_data = [("q", query), ("kl", "wt-wt")]; // wt-wt = no region redirect

    let response = client
        .post(LITE_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&form_data)
        .send()
        .await?;

    let status = response.status();
    if status != reqwest::StatusCode::OK {
        return Err(SearchError::HttpStatus {
            code: status.as_u16(),
            message: format!("DuckDuckGo Lite returned {status}"),
        });
    }

    let html = response.text().await.map_err(|e| {
        SearchError::Parse(format!("Failed to read DuckDuckGo Lite response: {e}"))
    })?;

    parse_lite_results(&html, query, config.max_results)
}

fn parse_lite_results(
    html: &str,
    query: &str,
    max_results: usize,
) -> Result<SearchResults, SearchError> {
    let document = Html::parse_document(html);

    // Lite 结果格式：
    // <table> 包含多行：
    // <tr class="result-snippet">  → 摘要
    // <tr class="result-link">    → URL（含 rel="nofollow" 的 <a>）
    // <tr class="result-sponsored"> → 广告（跳过）

    let link_selector =
        Selector::parse("a.result-link").map_err(|e| SearchError::Parse(e.to_string()))?;
    let snippet_selector =
        Selector::parse("td.result-snippet").map_err(|e| SearchError::Parse(e.to_string()))?;
    let title_selector =
        Selector::parse("a.result-link").map_err(|e| SearchError::Parse(e.to_string()))?;

    let mut results: Vec<SearchResult> = Vec::new();

    // 收集所有链接和摘要
    let links: Vec<_> = document.select(&link_selector).collect();
    let snippets: Vec<_> = document.select(&snippet_selector).collect();

    for (i, link_elem) in links.iter().enumerate() {
        if results.len() >= max_results {
            break;
        }

        let url = link_elem
            .value()
            .attr("href")
            .unwrap_or("")
            .to_string();

        let title = link_elem.text().collect::<Vec<_>>().join(" ").trim().to_string();

        // 尝试获取对应的摘要（snippet 数可能少于 link 数）
        let snippet = snippets
            .get(i)
            .map(|s| s.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();

        let clean_title = clean_html(&title);
        let clean_snippet = clean_html(&snippet);
        let clean_url = crate::html_utils::resolve_duckduckgo_url(&url);

        if clean_url.is_empty() || clean_url == "https:" {
            continue;
        }

        // 去重
        if results.iter().any(|r| r.url == clean_url) {
            continue;
        }

        results.push(SearchResult {
            title: if clean_title.is_empty() {
                clean_url.clone()
            } else {
                clean_title
            },
            url: clean_url,
            snippet: clean_snippet,
            source: SearchSource::DuckDuckGoLite,
            rank: results.len() + 1,
        });
    }

    if results.is_empty() {
        // 尝试从 title 选择器解析（fallback）
        for link_elem in document.select(&title_selector) {
            if results.len() >= max_results {
                break;
            }
            let url = link_elem
                .value()
                .attr("href")
                .unwrap_or("")
                .to_string();
            let title = link_elem.text().collect::<Vec<_>>().join(" ").trim().to_string();
            let clean_url = crate::html_utils::resolve_duckduckgo_url(&url);

            if clean_url.is_empty() || clean_url == "https:" {
                continue;
            }

            results.push(SearchResult {
                title: if title.is_empty() {
                    clean_url.clone()
                } else {
                    clean_html(&title)
                },
                url: clean_url,
                snippet: String::new(),
                source: SearchSource::DuckDuckGoLite,
                rank: results.len() + 1,
            });
        }
    }

    if results.is_empty() {
        return Err(SearchError::NoResults);
    }

    Ok(SearchResults {
        query: query.to_string(),
        results,
        source: SearchSource::DuckDuckGoLite,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lite_html() {
        let html = r#"<!DOCTYPE html>
<html>
<body>
<table>
<tr><td><a rel="nofollow" class="result-link" href="https://example.com">Example Title</a></td></tr>
<tr><td class="result-snippet">This is a snippet of the example page.</td></tr>
<tr><td><a rel="nofollow" class="result-link" href="https://rust-lang.org">Rust Programming Language</a></td></tr>
<tr><td class="result-snippet">A language empowering everyone to build reliable and efficient software.</td></tr>
</table>
</body>
</html>"#;

        let results = parse_lite_results(html, "test", 10).unwrap();
        assert_eq!(results.results.len(), 2);
        assert_eq!(results.results[0].title, "Example Title");
        assert_eq!(results.results[0].url, "https://example.com");
        assert_eq!(results.results[0].snippet, "This is a snippet of the example page.");
        assert_eq!(results.results[1].title, "Rust Programming Language");
    }

    #[test]
    fn test_parse_lite_empty() {
        let html = "<html><body>No results</body></html>";
        let result = parse_lite_results(html, "test", 10);
        assert!(result.is_err());
    }
}

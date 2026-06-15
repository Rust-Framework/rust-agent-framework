//! Bing CN 搜索后端（`cn.bing.com`）。
//!
//! 国内可访问的搜索引擎，适合在中国网络环境下使用。
//! 使用 `scraper` crate 解析 HTML，无需 API Key。

use crate::error::SearchError;
use crate::html_utils::clean_html;
use crate::types::{SearchConfig, SearchResult, SearchResults, SearchSource};
use scraper::{Html, Selector};

/// Bing CN 搜索 URL。
const BING_URL: &str = "https://cn.bing.com/search";

/// 通过 Bing CN 进行搜索。
pub async fn search_bing(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    let client = crate::anti_detection::build_client(config)?;

    let response = client
        .get(BING_URL)
        .query(&[("q", query), ("mkt", "zh-CN")])
        .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
        .send()
        .await?;

    let status = response.status();
    if status != reqwest::StatusCode::OK {
        return Err(SearchError::HttpStatus {
            code: status.as_u16(),
            message: format!("Bing CN returned {status}"),
        });
    }

    let html = response.text().await.map_err(|e| {
        SearchError::Parse(format!("Failed to read Bing CN response: {e}"))
    })?;

    parse_bing_results(&html, query, config.max_results)
}

/// 解析 Bing CN 搜索结果 HTML。
fn parse_bing_results(
    html: &str,
    query: &str,
    max_results: usize,
) -> Result<SearchResults, SearchError> {
    let document = Html::parse_document(html);

    // Bing 搜索结果结构：
    // <li class="b_algo">
    //   <h2><a href="URL">Title</a></h2>
    //   <div class="b_caption">
    //     <p class="b_lineclamp2">Snippet</p>  (或 b_lineclamp3, b_lineclamp4)
    //   </div>
    // </li>
    let algo_selector =
        Selector::parse("li.b_algo").map_err(|e| SearchError::Parse(e.to_string()))?;
    let link_selector =
        Selector::parse("h2 a").map_err(|e| SearchError::Parse(e.to_string()))?;
    let snippet_selector =
        Selector::parse(".b_caption p").map_err(|e| SearchError::Parse(e.to_string()))?;

    let mut results: Vec<SearchResult> = Vec::new();

    for algo_elem in document.select(&algo_selector) {
        if results.len() >= max_results {
            break;
        }

        // 提取标题和 URL
        let link = algo_elem.select(&link_selector).next();
        let (raw_url, raw_title) = match link {
            Some(elem) => {
                let url = elem.value().attr("href").unwrap_or("").to_string();
                let title = elem.text().collect::<Vec<_>>().join(" ");
                (url, title)
            }
            None => continue,
        };

        let url = raw_url.trim().to_string();
        let title = clean_html(&raw_title);

        // 跳过无效结果或广告
        if url.is_empty() || url.starts_with("javascript:") {
            continue;
        }

        // 提取摘要
        let snippet = algo_elem
            .select(&snippet_selector)
            .next()
            .map(|s| s.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let snippet = clean_html(&snippet);

        // 去重
        if results.iter().any(|r| r.url == url) {
            continue;
        }

        results.push(SearchResult {
            title: if title.is_empty() { url.clone() } else { title },
            url,
            snippet,
            source: SearchSource::BingCn,
            rank: results.len() + 1,
        });
    }

    if results.is_empty() {
        return Err(SearchError::NoResults);
    }

    Ok(SearchResults {
        query: query.to_string(),
        results,
        source: SearchSource::BingCn,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bing_results() {
        let html = r#"<!DOCTYPE html>
<html>
<body>
<ol id="b_results">
    <li class="b_algo">
        <h2><a href="https://example.com">Example Title</a></h2>
        <div class="b_caption">
            <p class="b_lineclamp2">This is a snippet about example.</p>
        </div>
    </li>
    <li class="b_algo">
        <h2><a href="https://rust-lang.org">Rust Programming Language</a></h2>
        <div class="b_caption">
            <p class="b_lineclamp2">A language empowering everyone to build reliable and efficient software.</p>
        </div>
    </li>
    <li class="b_algo">
        <h2><a href="https://example.com">Duplicate Title</a></h2>
        <div class="b_caption">
            <p class="b_lineclamp2">This should be skipped due to dedup.</p>
        </div>
    </li>
</ol>
</body>
</html>"#;

        let results = parse_bing_results(html, "test", 10).unwrap();
        assert_eq!(results.results.len(), 2);
        assert_eq!(results.results[0].title, "Example Title");
        assert_eq!(results.results[0].url, "https://example.com");
        assert_eq!(results.results[0].snippet, "This is a snippet about example.");
        assert_eq!(results.results[1].url, "https://rust-lang.org");
        assert_eq!(results.results[1].source, SearchSource::BingCn);
    }

    #[test]
    fn test_parse_bing_empty() {
        let html = "<html><body>No results found</body></html>";
        let result = parse_bing_results(html, "test", 10);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_bing_skip_ads() {
        let html = r#"<!DOCTYPE html>
<html>
<body>
<ol id="b_results">
    <li class="b_algo">
        <h2><a href="javascript:void(0)">Ad Placeholder</a></h2>
        <div class="b_caption"><p>Some ad text</p></div>
    </li>
</ol>
</body>
</html>"#;
        let result = parse_bing_results(html, "test", 10);
        assert!(result.is_err());
    }
}
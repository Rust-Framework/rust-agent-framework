use rust_agent_macros::tool;
use std::hash::{Hash, Hasher};

const DEFAULT_COUNT: i64 = 5;
const MAX_COUNT: i64 = 10;

/// 从环境变量构建 SearchConfig，支持运行时配置代理和 SearXNG 实例。
fn build_search_config(count: usize) -> rust_websearch::SearchConfig {
    let mut config = rust_websearch::SearchConfig::new(count);

    // 从环境变量读取代理配置
    if let Ok(proxy) = std::env::var("WEBSEARCH_PROXY_URL") {
        config.proxy_url = Some(proxy);
    }

    // 从环境变量读取 SearXNG 实例地址
    if let Ok(searxng) = std::env::var("WEBSEARCH_SEARXNG_URL") {
        config.searxng_url = Some(searxng);
    }

    config
}

#[tool(description = "Searches the web using DuckDuckGo/Bing/SearXNG multi-backend engine and returns a list of results with title, URL, and snippet. No API key required. Use this to find information and URLs, then use web_fetch(url) to get the full content of any result.")]
async fn web_search(
    #[param(desc = "Search query")] query: String,
    #[param(desc = "Maximum number of results to return (default: 5, max: 10)")] count: Option<i64>,
) -> String {
    let count = count
        .unwrap_or(DEFAULT_COUNT)
        .clamp(1, MAX_COUNT) as usize;

    let config = build_search_config(count);

    match rust_websearch::search(&query, &config).await {
        Ok(search_results) => {
            tracing::info!(
                query = %query,
                count = search_results.results.len(),
                source = ?search_results.source,
                "web_search succeeded"
            );
            let results: Vec<serde_json::Value> = search_results
                .results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "title": r.title,
                        "url": r.url,
                        "snippet": r.snippet,
                        "rank": r.rank,
                    })
                })
                .collect();

            // 生成指纹：基于所有结果 URL 计算哈希，帮助 LLM 识别重复结果
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for r in &search_results.results {
                r.url.hash(&mut hasher);
            }
            let fingerprint = hasher.finish();

            let result = serde_json::json!({
                "ok": true,
                "data": {
                    "query": query,
                    "results": results,
                    "count": results.len(),
                    "_source": format!("{:?}", search_results.source),
                    "_fingerprint": fingerprint,
                    "_tip": "Use web_fetch(url) to get full content from any URL above. If results don't change across calls (_fingerprint is same), try a more specific query or fetch a URL directly.",
                }
            });

            result.to_string()
        }
        Err(e) => {
            tracing::warn!(query = %query, error = %e, "web_search failed");
            let error_str = format!("{e}");
            let suggestion = if error_str.contains("No search results") || error_str.contains("NoResults") {
                format!("No results found. Try a different query, use simpler keywords, or try searching in English.")
            } else if error_str.contains("CAPTCHA") || error_str.contains("Rate limited") {
                format!("Search rate limited. Wait a moment and try again, or use a different query phrasing.")
            } else if error_str.contains("Timeout") || error_str.contains("Network") {
                format!("Search service temporarily unavailable. Try again in a moment, or use a more specific search query.")
            } else {
                format!("Search failed: {error_str}. Try a different query or check your network connection.")
            };

            serde_json::json!({
                "ok": false,
                "data": null,
                "error": format!("Search failed: {error_str}"),
                "suggestion": suggestion,
            })
            .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_web_search_empty_query() {
        let result = WebSearch.call(String::new(), None).await;
        assert!(result.contains("\"ok\":false"));
    }

    #[tokio::test]
    async fn test_web_search_name() {
        assert_eq!(WebSearch.name(), "web_search");
    }

    #[tokio::test]
    async fn test_web_search_desc() {
        assert!(!WebSearch.description().is_empty());
    }
}

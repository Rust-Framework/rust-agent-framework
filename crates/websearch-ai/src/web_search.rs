use rust_agent_macros::tool;
use std::hash::{Hash, Hasher};

const DEFAULT_COUNT: i64 = 5;
const MAX_COUNT: i64 = 10;

#[tool(description = "Searches the web and returns a list of results with title, URL, and snippet.")]
async fn web_search(
    #[param(desc = "Search query")] query: String,
    #[param(desc = "Maximum number of results to return (default: 5)")] count: Option<i64>,
) -> String {
    let count = count
        .unwrap_or(DEFAULT_COUNT)
        .clamp(1, MAX_COUNT) as usize;

    let config = rust_websearch::SearchConfig::new(count);

    match rust_websearch::search(&query, &config).await {
        Ok(search_results) => {
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

            serde_json::json!({
                "ok": true,
                "data": {
                    "query": query,
                    "results": results,
                    "count": results.len(),
                    "_source": format!("{:?}", search_results.source),
                    "_fingerprint": fingerprint,
                    "_tip": "Use web_fetch(url) to get full content from any URL above. If results don't change across calls (_fingerprint is same), try a more specific query or fetch a URL directly.",
                }
            })
            .to_string()
        }
        Err(e) => {
            let error_str = format!("{e}");
            let suggestion = if error_str.contains("No search results") {
                "Try a different or more specific query.".to_string()
            } else if error_str.contains("Timeout") || error_str.contains("Network error") || error_str.contains("Connection failed") {
                format!("Search backend is currently unreachable. You can use web_fetch(url) with a known URL to fetch content directly instead.")
            } else if error_str.contains("Rate limited") || error_str.contains("CAPTCHA") {
                format!("Search backend is rate-limited. Try again later or use web_fetch(url) with a known URL.")
            } else {
                format!("Try a different query or use web_fetch(url) with a known URL.")
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
    async fn test_web_search_basic() {
        let result = WebSearch
            .execute(serde_json::json!({"query": "rust programming language", "count": 3}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(v["ok"].as_bool().is_some());
        if v["ok"] == true {
            assert!(v["data"]["count"].as_u64().unwrap() > 0);
        }
    }
}

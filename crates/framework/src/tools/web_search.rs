use rust_agent_macros::tool;

use super::{err_response, ok_response};

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

    let mut engine = tarzi::SearchEngine::new();

    let search_results = match engine.search(&query, count).await {
        Ok(r) => r,
        Err(e) => return err_response(&format!("Search failed: {}", e)),
    };

    let results: Vec<serde_json::Value> = search_results
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

    // Explicit shutdown to release browser resources
    engine.shutdown().await;

    ok_response(serde_json::json!({
        "query": query,
        "results": results,
        "count": results.len(),
    }))
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
        // Network-dependent test: accept both ok and error responses
        // as long as the JSON is well-formed with expected fields
        assert!(v["ok"].as_bool().is_some());
        if v["ok"] == true {
            assert!(v["data"]["count"].as_u64().unwrap() > 0);
        }
    }
}

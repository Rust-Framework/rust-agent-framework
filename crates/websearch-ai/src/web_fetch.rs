use rust_agent_macros::tool;

#[tool(description = "Fetches content from a URL and returns it as plain text.")]
async fn web_fetch(
    #[param(desc = "The URL to fetch")] url: String,
) -> String {
    let config = rust_websearch::FetchConfig::default();

    match rust_websearch::fetch_page(&url, &config).await {
        Ok(page) => {
            serde_json::json!({
                "ok": true,
                "data": {
                    "url": page.url,
                    "final_url": page.final_url,
                    "title": page.title,
                    "content": page.content,
                    "content_length": page.content_length,
                    "truncated": page.truncated,
                    "status_code": page.status_code,
                }
            })
            .to_string()
        }
        Err(e) => {
            let error_str = format!("{e}");
            let suggestion = if error_str.contains("Timeout") || error_str.contains("Connection") {
                format!("The URL is unreachable. Check the URL and try again.")
            } else if error_str.contains("404") || error_str.contains("Not Found") {
                format!("The URL returned 404. Check if the URL is correct.")
            } else {
                format!("Check the URL and try again.")
            };
            serde_json::json!({
                "ok": false,
                "data": null,
                "error": format!("Fetch failed: {error_str}"),
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
    async fn test_web_fetch_invalid_url() {
        let result = WebFetch
            .execute(serde_json::json!({"url": "http://invalid.domain.that.does.not.exist.example"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
    }
}

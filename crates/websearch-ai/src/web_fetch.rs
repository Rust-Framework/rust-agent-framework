use rust_agent_macros::tool;

#[tool(description = "Fetches content from a URL and returns it as Markdown. Uses an embedded Servo browser engine for real JavaScript execution and layout-aware content extraction. Automatically strips navigation bars, footers, cookie banners, and ads. Supports Chinese encoding (GBK/GB2312/Big5) and SPA pages. Use settle_ms for JavaScript-heavy sites that need extra time to render.")]
async fn web_fetch(
    #[param(desc = "The URL to fetch")] url: String,
    #[param(desc = "Maximum content length in bytes (default: 50000)")] max_length: Option<usize>,
    #[param(desc = "Extra wait time in milliseconds after page load for SPA hydration (default: 0, max: 10000)")] settle_ms: Option<u64>,
) -> String {
    let mut config = rust_websearch::FetchConfig::default();
    if let Some(max_len) = max_length {
        config.max_content_bytes = max_len.clamp(1000, 200_000);
    }
    if let Some(ms) = settle_ms {
        config.settle_ms = ms.min(10_000);
    }

    match rust_websearch::fetch_page(&url, &config).await {
        Ok(page) => {
            let mut result = serde_json::json!({
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
            });

            if page.truncated {
                result["_suggestion"] = serde_json::Value::String(
                    "Content was truncated. Try fetching a more specific sub-page or use a smaller max_length.".into()
                );
            }

            result.to_string()
        }
        Err(e) => {
            let error_str = format!("{e}");
            let suggestion = if error_str.contains("Timeout") || error_str.contains("timeout") {
                format!("The page took too long to load. Try increasing settle_ms or check if the URL is correct.")
            } else if error_str.contains("Invalid URL") {
                format!("The URL is invalid. Check the URL format and try again.")
            } else if error_str.contains("not allowed") || error_str.contains("SSRF") {
                format!("The URL points to a private or reserved address that is blocked for security reasons.")
            } else if error_str.contains("Connection") || error_str.contains("unreachable") {
                format!("The URL is unreachable. Check the URL and try again.")
            } else {
                format!("Fetch failed: {error_str}. Check the URL and try again, or use web_search to find alternative sources.")
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
        let result = WebFetch.call("".to_string(), None).await;
        assert!(result.contains("\"ok\":false"));
    }

    #[tokio::test]
    async fn test_web_fetch_name() {
        assert_eq!(WebFetch.name(), "web_fetch");
    }

    #[tokio::test]
    async fn test_web_fetch_description() {
        assert!(!WebFetch.description().is_empty());
    }
}

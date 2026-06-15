use rust_agent_macros::tool;

#[tool(description = "Fetches content from a URL and returns it as plain text. Handles Chinese encoding (GBK/GB2312), automatically extracts main article content while filtering out navigation, ads, and sidebar noise. For JavaScript-heavy sites, the HTTP-only approach may not render dynamic content — use alternative sources if the returned content appears empty.")]
async fn web_fetch(
    #[param(desc = "The URL to fetch")] url: String,
    #[param(desc = "Maximum content length in bytes (default: 50000)")] max_length: Option<usize>,
) -> String {
    let mut config = rust_websearch::FetchConfig::default();
    if let Some(max_len) = max_length {
        config.max_content_bytes = max_len.clamp(1000, 200_000);
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

            // 如果内容为空或很短，提示可能是 JS 渲染页面
            if page.content.trim().len() < 100 {
                result["_suggestion"] = serde_json::Value::String(
                    "The page returned very little text content. This may be a JavaScript-rendered page. Try: 1) Use the page's sitemap or RSS feed, 2) Try an API endpoint if available, 3) Use a cached/text version of the page.".into()
                );
            }

            result.to_string()
        }
        Err(e) => {
            let error_str = format!("{e}");
            let suggestion = if error_str.contains("Timeout") || error_str.contains("Connection") {
                format!("The URL is unreachable. Check the URL and try again. If the site blocks crawlers, try using a cached version or alternative source.")
            } else if error_str.contains("404") || error_str.contains("Not Found") {
                format!("The URL returned 404. Check if the URL is correct and the page still exists.")
            } else if error_str.contains("too large") || error_str.contains("truncated") {
                format!("Content too large. Try fetching a more specific sub-page or API endpoint.")
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

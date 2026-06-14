use rust_agent_macros::tool;

use super::{err_response, ok_response};

const MAX_CONTENT_BYTES: usize = 50 * 1024; // 50 KB

#[tool(description = "Fetches content from a URL and returns it as Markdown text.")]
async fn web_fetch(
    #[param(desc = "The URL to fetch")] url: String,
) -> String {
    let mut fetcher = tarzi::WebFetcher::new();

    let content = match fetcher
        .fetch(&url, tarzi::FetchMode::PlainRequest, tarzi::Format::Markdown)
        .await
    {
        Ok(c) => c,
        Err(e) => return err_response(&format!("Fetch failed: {}", e)),
    };

    // Extract title from the Markdown content (first H1 heading)
    let title = content
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|l| l[2..].trim().to_string())
        .unwrap_or_default();

    let truncated = content.len() > MAX_CONTENT_BYTES;
    let display = if truncated {
        let end = content
            .char_indices()
            .take(MAX_CONTENT_BYTES)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        format!("{}...\n\n[truncated — content too large]", &content[..end])
    } else {
        content
    };

    ok_response(serde_json::json!({
        "url": url,
        "title": title,
        "content": display,
        "content_length": display.len(),
        "truncated": truncated,
    }))
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
        // Should be an error (network failure or timeout)
        assert_eq!(v["ok"], false);
    }
}

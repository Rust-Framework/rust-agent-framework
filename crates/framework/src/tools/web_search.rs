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

    // Try tarzi-based search first (requires ChromeDriver/GeckoDriver on Linux/macOS)
    // Fall back to DuckDuckGo HTML search when browser drivers are unavailable (e.g. Windows)
    let search_results = match search_via_tarzi(&query, count).await {
        Ok(results) => results,
        Err(_) => match search_via_duckduckgo(&query, count).await {
            Ok(results) => results,
            Err(e) => return err_response(&format!("Search failed: {}", e)),
        },
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

    ok_response(serde_json::json!({
        "query": query,
        "results": results,
        "count": results.len(),
    }))
}

// ── Data types ─────────────────────────────────────────────────────────

struct SearchResultItem {
    title: String,
    url: String,
    snippet: String,
    rank: usize,
}

// ── tarzi-based search (browser mode) ───────────────────────────────────

async fn search_via_tarzi(query: &str, count: usize) -> Result<Vec<SearchResultItem>, String> {
    let mut engine = tarzi::SearchEngine::new();

    let results = engine.search(query, count).await.map_err(|e| e.to_string())?;
    engine.shutdown().await;

    Ok(results
        .into_iter()
        .enumerate()
        .map(|(i, r)| SearchResultItem {
            title: r.title,
            url: r.url,
            snippet: r.snippet,
            rank: i + 1,
        })
        .collect())
}

// ── DuckDuckGo HTML search fallback (no browser/WebDriver needed) ───────

async fn search_via_duckduckgo(
    query: &str,
    count: usize,
) -> Result<Vec<SearchResultItem>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let response = client
        .get("https://html.duckduckgo.com/html/")
        .query(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let html = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {e}"))?;

    parse_duckduckgo_results(&html, count)
}

fn parse_duckduckgo_results(html: &str, max: usize) -> Result<Vec<SearchResultItem>, String> {
    use regex::Regex;

    let title_re = Regex::new(
        r###"<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"###,
    )
    .map_err(|e| format!("Regex error: {e}"))?;

    let snippet_re =
        Regex::new(r###"<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"###)
            .map_err(|e| format!("Regex error: {e}"))?;

    let mut items: Vec<SearchResultItem> = Vec::new();

    // Iterate over title matches in order; each title is expected to be
    // followed by a snippet `<a class="result__snippet">`
    for caps in title_re.captures_iter(html) {
        if items.len() >= max {
            break;
        }

        let raw_url = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let raw_title = caps.get(2).map(|m| m.as_str()).unwrap_or("");

        // Find the nearest snippet after this title
        let after_title = &html[caps.get(0).unwrap().end()..];
        let snippet = snippet_re
            .captures(after_title)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        let item = SearchResultItem {
            title: clean_html(&raw_title),
            url: resolve_duckduckgo_url(raw_url),
            snippet: clean_html(&snippet),
            rank: items.len() + 1,
        };

        // Deduplicate by URL
        if !items.iter().any(|i| i.url == item.url) {
            items.push(item);
        }
    }

    if items.is_empty() {
        Err("No search results returned by DuckDuckGo (possibly blocked or rate-limited).".into())
    } else {
        Ok(items)
    }
}

/// DuckDuckGo result URLs are often redirects via `//duckduckgo.com/l/?uddg=...`
/// or plain external URLs. Extract the actual target.
fn resolve_duckduckgo_url(url: &str) -> String {
    // Decode HTML entities first
    let decoded = decode_html_entities(url);

    // Handle DuckDuckGo redirect URLs
    if let Some(rest) = decoded.strip_prefix("/l/?uddg=") {
        // The actual URL is URL-encoded after uddg=
        urlencoding_decode(rest).unwrap_or_else(|| decoded.clone())
    } else if let Some(rest) = decoded.strip_prefix("//") {
        // Protocol-relative URL (e.g. //example.com/page)
        format!("https:{rest}")
    } else {
        decoded
    }
}

fn clean_html(input: &str) -> String {
    use regex::Regex;

    // Remove HTML tags
    let re = Regex::new(r"<[^>]*>").unwrap();
    let cleaned = re.replace_all(input, " ");
    // Collapse whitespace
    let re2 = Regex::new(r"\s+").unwrap();
    let result = re2.replace_all(&cleaned, " ");

    let result = result.trim().to_string();
    decode_html_entities(&result)
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Minimal URL-decode for `%XX` sequences (used for DuckDuckGo redirect URLs).
fn urlencoding_decode(input: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let hi = chars.next()?;
            let lo = chars.next()?;
            let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).ok()?;
            bytes.push(byte);
        } else if ch == '+' {
            bytes.push(b' ');
        } else {
            // Only keep ASCII characters; skip non-ASCII for simplicity
            if ch.is_ascii() {
                bytes.push(ch as u8);
            }
        }
    }

    String::from_utf8(bytes).ok()
}

// ── Tests ──────────────────────────────────────────────────────────────

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
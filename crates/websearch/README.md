# rust-websearch

Pure Rust web search library — DuckDuckGo multi-backend, SearXNG client, Bing support, with anti-detection and Servo-based page fetching. **No API key required.**

## Quick Start

```rust
use rust_websearch::{search, SearchConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = SearchConfig::default();
    let results = search("rust programming", &config).await?;
    println!("Found {} results from {:?}", results.results.len(), results.source);
    Ok(())
}
```

## Search Backends (No API Key Required)

| Backend | URL | Description |
|---|---|---|
| DuckDuckGo Lite | `lite.duckduckgo.com` | Preferred — lightest payload, least anti-crawl |
| DuckDuckGo HTML | `html.duckduckgo.com` | General web search |
| DuckDuckGo Instant Answer | `api.duckduckgo.com` | JSON API, best for knowledge queries |
| Bing CN | `cn.bing.com` | Alternative for regions where DuckDuckGo is slow |
| SearXNG | Self-hosted | Aggregates 70+ search engines; configure via `SEARXNG_URL` |

## Public API

### Core Functions

| Function | Description |
|---|---|
| `search(query, config)` | Run a search using auto-selected backend |
| `search_searxng(query, instance_url)` | Search via a specific SearXNG instance |
| `search_bing(query, config)` | Search via Bing CN |
| `fetch_page(url, config)` | Fetch and render a page with Servo browser engine |

### Key Types

| Type | Description |
|---|---|
| `SearchConfig` | Search configuration: `max_results`, `timeout_secs`, `language`, `proxy_url` |
| `SearchResults` | Search response: `results: Vec<SearchResult>`, `source: SearchSource` |
| `SearchResult` | Single result: `title`, `url`, `snippet`, `rank` |
| `SearchSource` | Enum: `DuckDuckGoLite`, `DuckDuckGoHtml`, `DuckDuckGoInstantAnswer`, `BingCN`, `SearXNG` |
| `FetchConfig` | Fetch configuration: `max_content_length`, `settle_ms`, `clean_mode`, `proxy_url` |
| `FetchedPage` | Fetched page: `url`, `final_url`, `title`, `content` (Markdown), `status_code` |
| `CleanMode` | Content cleaning mode: `Default`, `Minimal`, `Full` |

### Anti-Detection

| Function | Description |
|---|---|
| `random_user_agent()` | Return a random User-Agent string from the pool |
| `RateLimiter` | Rate limiter with configurable delay and random jitter |

### Content Cleaning

| Type | Description |
|---|---|
| `ContentCleaner` | HTML content cleaner removing navigation, footers, ads, cookie banners |
| `score_content(html)` | Score HTML content blocks for relevance (higher = more likely main content) |

### Error Handling

| Type | Description |
|---|---|
| `SearchError` | Unified error type: `HttpError`, `ParseError`, `RateLimited`, `Timeout`, `NoResults`, etc. |

### Backend Probing

| Function | Description |
|---|---|
| `Reachability` | Backend reachability status |
| `BackendKind` | Enum of available backends |
| `clear_probe_cache()` | Clear internal backend reachability cache |

## Page Fetching (Servo Engine)

The `fetch_page()` function uses the [Servo](https://servo.org/) browser engine for:

- **Real JavaScript execution** — SpiderMonkey JS engine, supports SPA hydration
- **Layout-aware extraction** — DOM-based content extraction removes navigation, footers, ads
- **Markdown output** — Extracted content is converted to clean Markdown
- **SPA settle time** — Configurable `settle_ms` to wait for dynamic content
- **Security** — SSRF protection: blocks internal IPs and reserved addresses

```rust
use rust_websearch::{fetch_page, FetchConfig};

let config = FetchConfig {
    max_content_length: 50000,
    settle_ms: 2000,  // wait 2s for SPA hydration
    ..Default::default()
};

let page = fetch_page("https://example.com", &config).await?;
println!("Title: {}", page.title);
println!("Content (Markdown):\n{}", &page.content[..200.min(page.content.len())]);
```

## Configuration

### SearchConfig

```rust
let config = SearchConfig {
    max_results: 10,           // max results to return
    timeout_secs: 30,          // request timeout
    language: Some("zh-CN".into()), // search language preference
    proxy_url: None,           // optional HTTP/SOCKS5 proxy
};
```

### Environment Variables

| Variable | Description | Example |
|---|---|---|
| `WEBSEARCH_PROXY_URL` | HTTP/SOCKS5 proxy URL | `http://127.0.0.1:7890` or `socks5://127.0.0.1:1080` |
| `SEARXNG_URL` | Self-hosted SearXNG instance URL | `https://searx.example.com` |

## Feature Flags

| Feature | Default | Description |
|---|---|---|
| `rustls-tls` | **Yes** | Use rustls for TLS (reqwest) |
| `native-tls` | No | Use platform-native TLS (reqwest) |

## Dependencies

| Crate | Purpose |
|---|---|
| `reqwest` | HTTP client for search backends |
| `servo-fetch` | Servo browser engine for JS execution and layout-aware page fetching |
| `scraper` | CSS-selector-based HTML parsing for search results |
| `rand` | User-Agent rotation and jitter |
| `url` | URL parsing and normalization |
| `regex` | Content cleaning and text extraction |
| `serde` / `serde_json` | Serialization |

## Relationship to rust-agent-websearch

This crate is the **low-level search engine**. For Agent tool integration (as `#[tool]` implementations with `IContextProvider` support), see [`rust-agent-websearch`](../websearch-ai/).

use rust_agent_macros::tool;

/// 从共享配置或环境变量构建 FetchConfig，支持运行时配置代理。
/// 优先级：共享配置 > 环境变量。
pub(crate) fn build_fetch_config() -> rust_websearch::FetchConfig {
    let mut config = rust_websearch::FetchConfig::default();

    // 优先从共享配置读取
    if let Some(shared) = crate::get_shared_config() {
        config.proxy_url.clone_from(&shared.proxy_url);
    }

    // 回退到环境变量
    if config.proxy_url.is_none() {
        if let Ok(proxy) = std::env::var("WEBSEARCH_PROXY_URL") {
            config.proxy_url = Some(proxy);
        }
    }

    config
}

#[tool(description = "获取指定 URL 的内容并转换为 Markdown 格式。", kind = "web")]
async fn web_fetch(
    #[param(desc = "要获取的 URL")] url: String,
    #[param(desc = "最大内容长度（字节，默认: 50000）")] max_length: Option<usize>,
    #[param(desc = "页面加载后额外等待的毫秒数，用于 SPA 页面渲染（默认: 0，最大: 10000）")] settle_ms: Option<u64>,
    #[param(desc = "内容清洗模式：'auto'（默认）、'aggressive'（激进）、'minimal'（最小）、'raw'（原始）")] clean_mode: Option<String>,
) -> rust_agent_core::ToolResult {
    let mut config = build_fetch_config();
    if let Some(max_len) = max_length {
        config.max_content_bytes = max_len.clamp(1000, 200_000);
    }
    if let Some(ms) = settle_ms {
        config.settle_ms = ms.min(10_000);
    }
    if let Some(mode_str) = clean_mode {
        if let Some(mode) = rust_websearch::CleanMode::from_str(&mode_str) {
            config.clean_mode = mode;
        }
    }

    match rust_websearch::fetch_page(&url, &config).await {
        Ok(page) => {
            tracing::info!(
                url = %url,
                title = %page.title,
                content_len = page.content_length,
                "web_fetch succeeded"
            );
            let mut result = rust_agent_core::ToolResult::success(serde_json::json!({
                "url": page.url,
                "final_url": page.final_url,
                "title": page.title,
                "content": page.content,
                "content_length": page.content_length,
                "truncated": page.truncated,
                "status_code": page.status_code,
            }));
            if page.truncated {
                result = rust_agent_core::ToolResult::success(serde_json::json!({
                    "url": page.url,
                    "final_url": page.final_url,
                    "title": page.title,
                    "content": page.content,
                    "content_length": page.content_length,
                    "truncated": page.truncated,
                    "status_code": page.status_code,
                    "_suggestion": "Content was truncated. Try fetching a more specific sub-page or use a smaller max_length.",
                }));
            }
            result
        }
        Err(e) => {
            tracing::warn!(url = %url, error = %e, "web_fetch failed");
            let error_str = format!("{e}");
            let suggestion = if error_str.contains("Timeout") || error_str.contains("timeout") {
                "The page took too long to load. Try increasing settle_ms or check if the URL is correct."
            } else if error_str.contains("Invalid URL") {
                "The URL is invalid. Check the URL format and try again."
            } else if error_str.contains("not allowed") || error_str.contains("SSRF") {
                "The URL points to a private or reserved address that is blocked for security reasons."
            } else if error_str.contains("Connection") || error_str.contains("unreachable") {
                "The URL is unreachable. Check the URL and try again."
            } else {
                "Fetch failed. Check the URL and try again, or use web_search to find alternative sources."
            };
            rust_agent_core::ToolResult::error_with_data(
                format!("Fetch failed: {error_str}"),
                serde_json::json!({"suggestion": suggestion}),
            )
        }
    }
}

//! 搜索协调器 —— 智能后端选择 + 降级链。
//!
//! `search()` 先通过网络探测快速识别可达的后端，
//! 然后仅对可达的后端按优先级尝试搜索，避免无谓的超时等待。

use crate::anti_detection::RateLimiter;
use crate::duckduckgo;
use crate::error::SearchError;
use crate::probe;
use crate::types::{SearchConfig, SearchResults};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// 全局速率限制器（跨所有搜索请求共享）。
fn global_rate_limiter() -> &'static Arc<RateLimiter> {
    static LIMITER: std::sync::OnceLock<Arc<RateLimiter>> = std::sync::OnceLock::new();
    LIMITER.get_or_init(|| Arc::new(RateLimiter::new()))
}

/// 执行搜索。
///
/// # 智能选路策略
///
/// 当 `probe_timeout_ms > 0` 时：
///   1. 快速探测各后端可达性（结果缓存 30 秒）
///   2. 按优先级仅尝试 **可达** 的后端
///   3. 若全部不可达，抛出 `NoResults`（避免漫长超时）
///
/// 当 `probe_timeout_ms == 0` 时（传统模式）：
///   按固定顺序逐个尝试所有后端。
pub async fn search(query: &str, config: &SearchConfig) -> Result<SearchResults, SearchError> {
    if query.trim().is_empty() {
        return Err(SearchError::Config("Search query cannot be empty".into()));
    }

    // 速率控制
    global_rate_limiter().wait(config.min_interval_ms).await;

    // ── 智能探测：识别可达后端 ──
    if config.probe_timeout_ms > 0 {
        let probe_results = probe::probe_all(config).await;

        let ddg_ok = probe::duckduckgo_reachable(&probe_results);
        let bing_ok = probe::bing_cn_reachable(&probe_results);
        let searxng_ok = probe::searxng_reachable(&probe_results);

        info!(
            "Network probe: DuckDuckGo={}, BingCN={}, SearXNG={}",
            if ddg_ok { "OK" } else { "BLOCKED" },
            if bing_ok { "OK" } else { "BLOCKED" },
            if searxng_ok { "OK" } else { "BLOCKED" },
        );

        // 优先使用 DuckDuckGo（反爬最轻量），其次 Bing CN，最后 SearXNG
        if ddg_ok {
            if let Ok(results) = try_duckduckgo(query, config).await {
                return Ok(results);
            }
        }

        if bing_ok {
            if let Ok(results) = crate::bing::search_bing(query, config).await {
                debug!("Bing CN succeeded: {} results", results.results.len());
                return Ok(results);
            }
        }

        if searxng_ok {
            if let Ok(results) = crate::searxng::search_searxng(query, config).await {
                debug!("SearXNG succeeded: {} results", results.results.len());
                return Ok(results);
            }
        }

        return Err(SearchError::NoResults);
    }

    // ── 传统模式（probe 禁用） ──
    fallback_search(query, config).await
}

/// 依次尝试 DuckDuckGo 的三个后端（Lite → Instant Answer → HTML）。
async fn try_duckduckgo(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    // Lite
    match duckduckgo::search_lite(query, config).await {
        Ok(results) => {
            debug!("DuckDuckGo Lite succeeded: {} results", results.results.len());
            return Ok(results);
        }
        Err(e) => debug!("DuckDuckGo Lite failed, trying next: {e}"),
    }

    // Instant Answer
    match duckduckgo::search_instant_answer(query, config).await {
        Ok(results) => {
            debug!(
                "DuckDuckGo Instant Answer succeeded: {} results",
                results.results.len()
            );
            return Ok(results);
        }
        Err(e) => debug!("DuckDuckGo Instant Answer failed, trying next: {e}"),
    }

    // HTML
    match duckduckgo::search_html(query, config).await {
        Ok(results) => {
            debug!("DuckDuckGo HTML succeeded: {} results", results.results.len());
            return Ok(results);
        }
        Err(e) => debug!("DuckDuckGo HTML failed: {e}"),
    }

    Err(SearchError::NoResults)
}

/// 传统降级链（固定顺序尝试所有后端）。
async fn fallback_search(
    query: &str,
    config: &SearchConfig,
) -> Result<SearchResults, SearchError> {
    // 1. DuckDuckGo
    match try_duckduckgo(query, config).await {
        Ok(results) => return Ok(results),
        Err(_) => debug!("All DuckDuckGo backends failed, trying Bing CN"),
    }

    // 2. Bing CN
    match crate::bing::search_bing(query, config).await {
        Ok(results) => {
            debug!("Bing CN succeeded: {} results", results.results.len());
            return Ok(results);
        }
        Err(e) => debug!("Bing CN failed, trying SearXNG: {e}"),
    }

    // 3. SearXNG
    if config.searxng_url.is_some() {
        match crate::searxng::search_searxng(query, config).await {
            Ok(results) => {
                debug!("SearXNG succeeded: {} results", results.results.len());
                return Ok(results);
            }
            Err(e) => debug!("SearXNG failed: {e}"),
        }
    }

    warn!("All search backends failed for query: {:?}", query);
    Err(SearchError::NoResults)
}
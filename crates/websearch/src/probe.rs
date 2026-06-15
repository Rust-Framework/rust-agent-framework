//! 网络探测模块 —— 智能识别各搜索引擎后端的可达性。
//!
//! 在发起实际搜索前，用短超时快速探测各后端的网络连通性，
//! 避免因某个后端不可达而长时间等待超时降级。
//!
//! 探测结果会缓存一段时间（默认 30 秒），减少重复探测。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

// ── 类型定义 ──

/// 搜索引擎后端类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// DuckDuckGo 系列（lite / instant answer / html 共享同一域名探测）
    DuckDuckGo,
    /// Bing 中国站（cn.bing.com）
    BingCn,
    /// 自建 SearXNG 实例
    SearXNG,
}

impl BackendKind {
    /// 返回该后端对应的探测 URL。
    fn probe_url(&self) -> &'static str {
        match self {
            BackendKind::DuckDuckGo => "https://duckduckgo.com/",
            BackendKind::BingCn => "https://cn.bing.com/",
            BackendKind::SearXNG => unreachable!("SearXNG URL must be provided at runtime"),
        }
    }
}

/// 后端可达性状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    /// 可达
    Reachable,
    /// 不可达（超时 / 连接拒绝 / DNS 解析失败等）
    Unreachable,
}

/// 探测结果项。
struct ProbeEntry {
    reachability: Reachability,
    probed_at: Instant,
}

/// 探测结果缓存。
struct ProbeCache {
    entries: HashMap<BackendKind, ProbeEntry>,
}

// ── 全局缓存 ──

/// 缓存默认 TTL（秒）。
const CACHE_TTL_SECS: u64 = 30;

static PROBE_CACHE: std::sync::OnceLock<Mutex<ProbeCache>> = std::sync::OnceLock::new();

fn probe_cache() -> &'static Mutex<ProbeCache> {
    PROBE_CACHE.get_or_init(|| {
        Mutex::new(ProbeCache {
            entries: HashMap::new(),
        })
    })
}

// ── 公共 API ──

/// 探测所有后端，返回各后端的可达性映射。
///
/// 对于每个后端，先查缓存（未过期 → 直接返回），
/// 否则发起一次短超时 HTTP HEAD 请求。
///
/// 多个后端**并行探测**，总耗时 ≈ 最慢单个后端的探测耗时。
pub async fn probe_all(config: &crate::types::SearchConfig) -> HashMap<BackendKind, Reachability> {
    let now = Instant::now();
    let probe_timeout = if config.probe_timeout_ms > 0 {
        Duration::from_millis(config.probe_timeout_ms)
    } else {
        Duration::from_secs(3)
    };

    // 1. 检查缓存，确定需要探测的后端
    let mut to_probe: Vec<BackendKind> = Vec::new();

    // DuckDuckGo
    if should_probe(BackendKind::DuckDuckGo) {
        to_probe.push(BackendKind::DuckDuckGo);
    }
    // Bing CN
    if should_probe(BackendKind::BingCn) {
        to_probe.push(BackendKind::BingCn);
    }
    // SearXNG
    if config.searxng_url.is_some() {
        if should_probe(BackendKind::SearXNG) {
            to_probe.push(BackendKind::SearXNG);
        }
    }

    // 2. 并行探测所有后端
    if !to_probe.is_empty() {
        let proxy = config.proxy_url.as_deref();
        let searxng_url = config.searxng_url.as_deref();

        let results = futures_util::future::join_all(
            to_probe
                .iter()
                .map(|&kind| probe_one(kind, searxng_url, proxy, probe_timeout)),
        )
        .await;

        // 3. 更新缓存
        if let Ok(mut cache) = probe_cache().lock() {
            for (kind, result) in to_probe.into_iter().zip(results.into_iter()) {
                cache.entries.insert(
                    kind,
                    ProbeEntry {
                        reachability: result,
                        probed_at: now,
                    },
                );
            }
        }
    }

    // 4. 从缓存构建最终结果
    let mut result = HashMap::new();
    if let Ok(cache) = probe_cache().lock() {
        for (&kind, entry) in &cache.entries {
            result.insert(kind, entry.reachability);
        }
    }

    // 对于未缓存的（极少情况）默认认为不可达
    for &kind in &[
        BackendKind::DuckDuckGo,
        BackendKind::BingCn,
        BackendKind::SearXNG,
    ] {
        result.entry(kind).or_insert(Reachability::Unreachable);
    }

    result
}

/// 探测单个后端。
async fn probe_one(
    kind: BackendKind,
    searxng_url: Option<&str>,
    proxy_url: Option<&str>,
    timeout: Duration,
) -> Reachability {
    let url = match kind {
        BackendKind::SearXNG => match searxng_url {
            Some(u) => u,
            None => return Reachability::Unreachable,
        },
        _ => kind.probe_url(),
    };

    // 使用短超时构建客户端
    let mut client_builder = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(crate::anti_detection::random_user_agent())
        .redirect(reqwest::redirect::Policy::limited(2));

    if let Some(proxy_str) = proxy_url {
        if let Ok(p) = reqwest::Proxy::all(proxy_str) {
            client_builder = client_builder.proxy(p);
        }
    }

    let client = match client_builder.build() {
        Ok(c) => c,
        Err(_) => return Reachability::Unreachable,
    };

    // 只发 HEAD 请求，比 GET 更轻量
    match client.head(url).send().await {
        Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
            Reachability::Reachable
        }
        // 某些服务器不支持 HEAD，降级到 GET 仅读头
        Ok(_) => match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() || resp.status().is_redirection() => {
                Reachability::Reachable
            }
            _ => Reachability::Unreachable,
        },
        Err(_) => Reachability::Unreachable,
    }
}

/// 检查某个后端是否需要重新探测（缓存缺失或已过期）。
fn should_probe(kind: BackendKind) -> bool {
    if let Ok(cache) = probe_cache().lock() {
        match cache.entries.get(&kind) {
            Some(entry) => {
                let elapsed = entry.probed_at.elapsed();
                elapsed >= Duration::from_secs(CACHE_TTL_SECS)
            }
            None => true,
        }
    } else {
        true
    }
}

/// 获取某个后端当前的缓存可达性（不触发新探测）。
pub fn cached_reachability(kind: BackendKind) -> Option<Reachability> {
    if let Ok(cache) = probe_cache().lock() {
        cache.entries.get(&kind).map(|e| e.reachability)
    } else {
        None
    }
}

/// 根据探测结果判断 DuckDuckGo 系列是否整体可达。
pub fn duckduckgo_reachable(probe_results: &HashMap<BackendKind, Reachability>) -> bool {
    probe_results
        .get(&BackendKind::DuckDuckGo)
        .copied()
        .unwrap_or(Reachability::Unreachable)
        == Reachability::Reachable
}

/// 根据探测结果判断 Bing CN 是否可达。
pub fn bing_cn_reachable(probe_results: &HashMap<BackendKind, Reachability>) -> bool {
    probe_results
        .get(&BackendKind::BingCn)
        .copied()
        .unwrap_or(Reachability::Unreachable)
        == Reachability::Reachable
}

/// 根据探测结果判断 SearXNG 是否可达。
pub fn searxng_reachable(probe_results: &HashMap<BackendKind, Reachability>) -> bool {
    probe_results
        .get(&BackendKind::SearXNG)
        .copied()
        .unwrap_or(Reachability::Unreachable)
        == Reachability::Reachable
}

/// 清除探测缓存（强制下次重新探测）。
pub fn clear_cache() {
    if let Ok(mut cache) = probe_cache().lock() {
        cache.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_empty_should_probe() {
        clear_cache();
        assert!(should_probe(BackendKind::DuckDuckGo));
    }

    #[test]
    fn test_cache_fresh_should_not_probe() {
        clear_cache();
        // 手动插入缓存
        if let Ok(mut cache) = probe_cache().lock() {
            cache.entries.insert(
                BackendKind::DuckDuckGo,
                ProbeEntry {
                    reachability: Reachability::Reachable,
                    probed_at: Instant::now(),
                },
            );
        }
        assert!(!should_probe(BackendKind::DuckDuckGo));
    }

    #[test]
    fn test_duckduckgo_reachable() {
        let mut map = HashMap::new();
        map.insert(BackendKind::DuckDuckGo, Reachability::Reachable);
        assert!(duckduckgo_reachable(&map));
        assert!(!bing_cn_reachable(&map));
    }

    #[test]
    fn test_all_unreachable_default() {
        let map = HashMap::new();
        assert!(!duckduckgo_reachable(&map));
        assert!(!bing_cn_reachable(&map));
        assert!(!searxng_reachable(&map));
    }
}
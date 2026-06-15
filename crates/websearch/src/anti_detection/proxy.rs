//! 代理管理器 —— 支持 HTTP/SOCKS5 代理池轮换。

use crate::error::SearchError;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use url::Url;

/// 支持的代理协议。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks5,
}

impl ProxyProtocol {
    fn from_url(url: &Url) -> Option<Self> {
        match url.scheme() {
            "http" => Some(ProxyProtocol::Http),
            "https" => Some(ProxyProtocol::Https),
            "socks5" | "socks5h" => Some(ProxyProtocol::Socks5),
            _ => None,
        }
    }
}

/// 代理条目。
#[derive(Debug)]
struct ProxyEntry {
    url: String,
    protocol: ProxyProtocol,
    /// 连续失败次数
    failures: AtomicUsize,
}

impl Clone for ProxyEntry {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            protocol: self.protocol.clone(),
            failures: AtomicUsize::new(self.failures.load(Ordering::Relaxed)),
        }
    }
}

/// 代理管理器（Round-Robin 轮换 + 故障自动摘除）。
#[derive(Debug)]
pub struct ProxyManager {
    proxies: Mutex<Vec<ProxyEntry>>,
    current_index: AtomicUsize,
    /// 连续失败 N 次后摘除代理
    max_failures: usize,
}

impl ProxyManager {
    pub fn new(max_failures: usize) -> Self {
        Self {
            proxies: Mutex::new(Vec::new()),
            current_index: AtomicUsize::new(0),
            max_failures,
        }
    }

    pub fn add_proxy(&self, proxy_url: &str) -> Result<(), String> {
        let url = Url::parse(proxy_url).map_err(|e| format!("Invalid proxy URL: {e}"))?;
        let protocol = ProxyProtocol::from_url(&url)
            .ok_or_else(|| format!("Unsupported proxy protocol: {}", url.scheme()))?;

        let mut proxies = self.proxies.lock().unwrap();
        proxies.push(ProxyEntry {
            url: proxy_url.to_string(),
            protocol,
            failures: AtomicUsize::new(0),
        });
        Ok(())
    }

    pub fn add_proxies(&self, urls: &[&str]) -> Result<(), String> {
        for url in urls {
            self.add_proxy(url)?;
        }
        Ok(())
    }

    pub fn next_proxy(&self) -> Option<String> {
        let proxies = self.proxies.lock().unwrap();
        if proxies.is_empty() {
            return None;
        }

        let len = proxies.len();
        for _ in 0..len {
            let idx = self.current_index.fetch_add(1, Ordering::Relaxed) % len;
            let entry = &proxies[idx];

            if entry.failures.load(Ordering::Relaxed) < self.max_failures {
                return Some(entry.url.clone());
            }
        }

        None
    }

    pub fn report_success(&self, proxy_url: &str) {
        let proxies = self.proxies.lock().unwrap();
        for entry in proxies.iter() {
            if entry.url == proxy_url {
                entry.failures.store(0, Ordering::Relaxed);
                return;
            }
        }
    }

    pub fn report_failure(&self, proxy_url: &str) {
        let proxies = self.proxies.lock().unwrap();
        for entry in proxies.iter() {
            if entry.url == proxy_url {
                entry.failures.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.proxies.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub fn build_proxied_client(
    proxy_url: &str,
    timeout_secs: u64,
) -> Result<reqwest::Client, SearchError> {
    let proxy = reqwest::Proxy::all(proxy_url)
        .map_err(|e| SearchError::Config(format!("Invalid proxy URL: {e}")))?;

    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .user_agent(crate::anti_detection::random_user_agent())
        .redirect(reqwest::redirect::Policy::limited(5))
        .proxy(proxy)
        .build()
        .map_err(|e| SearchError::Config(format!("Failed to build proxied HTTP client: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_manager_round_robin() {
        let mgr = ProxyManager::new(3);
        mgr.add_proxy("http://proxy1:8080").unwrap();
        mgr.add_proxy("http://proxy2:8080").unwrap();

        assert_eq!(mgr.next_proxy().unwrap(), "http://proxy1:8080");
        assert_eq!(mgr.next_proxy().unwrap(), "http://proxy2:8080");
        assert_eq!(mgr.next_proxy().unwrap(), "http://proxy1:8080");
    }

    #[test]
    fn test_proxy_manager_remove_on_failure() {
        let mgr = ProxyManager::new(2);
        mgr.add_proxy("http://bad-proxy:8080").unwrap();
        mgr.add_proxy("http://good-proxy:8080").unwrap();

        mgr.report_failure("http://bad-proxy:8080");
        mgr.report_failure("http://bad-proxy:8080");

        for _ in 0..5 {
            assert_eq!(mgr.next_proxy().unwrap(), "http://good-proxy:8080");
        }
    }

    #[test]
    fn test_proxy_manager_empty() {
        let mgr = ProxyManager::new(2);
        assert!(mgr.next_proxy().is_none());
    }
}

//! Cookie 管理器 —— 基于 `cookie_store` 实现会话持久化。
//!
//! 自动从响应中提取 Set-Cookie，在后续请求中注入匹配的 Cookie。

use cookie_store::CookieStore;
use std::sync::Mutex;
use url::Url;

/// 线程安全的 Cookie 管理器。
#[derive(Debug)]
pub struct CookieManager {
    store: Mutex<CookieStore>,
}

impl CookieManager {
    /// 创建新的 Cookie 管理器。
    pub fn new() -> Self {
        Self {
            store: Mutex::new(CookieStore::default()),
        }
    }

    /// 从 HTTP 响应中提取 Cookie 并存入管理器。
    ///
    /// `url` 是发出请求的 URL，`headers` 是响应头。
    pub fn store_response_cookies(&self, url: &str, headers: &reqwest::header::HeaderMap) {
        let Ok(parsed_url) = Url::parse(url) else {
            return;
        };

        let mut store = self.store.lock().unwrap();

        for (name, value) in headers.iter() {
            if name.as_str().eq_ignore_ascii_case("set-cookie") {
                if let Ok(cookie_str) = value.to_str() {
                    // cookie_store 的 store_response_cookies 需要请求 URL 和响应头
                    // 这里我们简化处理，直接插入 cookie
                    if let Ok(cookie) = cookie_store::RawCookie::parse(cookie_str) {
                        let cookie = cookie.into_owned();
                        let _ = store.insert_raw(&cookie, &parsed_url);
                    }
                }
            }
        }
    }

    /// 获取适用于给定 URL 的 Cookie 值字符串（`; ` 分隔）。
    pub fn get_cookie_header(&self, url: &str) -> Option<String> {
        let Ok(parsed_url) = Url::parse(url) else {
            return None;
        };

        let store = self.store.lock().unwrap();
        let cookies: Vec<String> = store
            .get_request_values(&parsed_url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect();

        if cookies.is_empty() {
            None
        } else {
            Some(cookies.join("; "))
        }
    }

    /// 清空所有 Cookie。
    pub fn clear(&self) {
        let mut store = self.store.lock().unwrap();
        *store = CookieStore::default();
    }
}

impl Default for CookieManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookie_manager_basic() {
        let mgr = CookieManager::new();

        // 模拟存储 cookie
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::SET_COOKIE,
            reqwest::header::HeaderValue::from_static("session_id=abc123; Path=/"),
        );

        mgr.store_response_cookies("https://example.com/", &headers);

        let cookie_header = mgr.get_cookie_header("https://example.com/");
        assert_eq!(cookie_header.unwrap(), "session_id=abc123");
    }

    #[test]
    fn test_cookie_manager_empty() {
        let mgr = CookieManager::new();
        assert!(mgr.get_cookie_header("https://example.com").is_none());
    }
}

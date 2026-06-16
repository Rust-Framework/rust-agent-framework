//! 搜索错误类型定义。

use std::fmt;

/// 搜索过程中的错误类型。
#[derive(Debug)]
pub enum SearchError {
    /// 网络请求失败
    Network(String),
    /// 被限流 / 触发反爬
    RateLimited(String),
    /// 触发 CAPTCHA 验证
    Captcha(String),
    /// HTTP 状态码异常
    HttpStatus { code: u16, message: String },
    /// 解析响应失败
    Parse(String),
    /// 所有后端均无结果
    NoResults,
    /// 配置错误
    Config(String),
    /// 超时
    Timeout(String),
    /// 其他错误
    Other(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::Network(msg) => write!(f, "Network error: {msg}"),
            SearchError::RateLimited(msg) => write!(f, "Rate limited: {msg}"),
            SearchError::Captcha(msg) => write!(f, "CAPTCHA triggered: {msg}"),
            SearchError::HttpStatus { code, message } => {
                write!(f, "HTTP {code}: {message}")
            }
            SearchError::Parse(msg) => write!(f, "Parse error: {msg}"),
            SearchError::NoResults => write!(f, "No search results from any backend"),
            SearchError::Config(msg) => write!(f, "Configuration error: {msg}"),
            SearchError::Timeout(msg) => write!(f, "Timeout: {msg}"),
            SearchError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<reqwest::Error> for SearchError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            SearchError::Timeout(e.to_string())
        } else if e.is_connect() {
            SearchError::Network(format!("Connection failed: {e}"))
        } else {
            SearchError::Network(e.to_string())
        }
    }
}

impl From<serde_json::Error> for SearchError {
    fn from(e: serde_json::Error) -> Self {
        SearchError::Parse(format!("JSON error: {e}"))
    }
}

impl From<url::ParseError> for SearchError {
    fn from(e: url::ParseError) -> Self {
        SearchError::Parse(format!("URL parse error: {e}"))
    }
}

impl From<servo_fetch::Error> for SearchError {
    fn from(e: servo_fetch::Error) -> Self {
        match &e {
            servo_fetch::Error::Timeout { url, timeout } => {
                SearchError::Timeout(format!("Page load timeout for {url} ({timeout:?})"))
            }
            servo_fetch::Error::InvalidUrl { url, reason } => {
                SearchError::Config(format!("Invalid URL {url}: {reason}"))
            }
            servo_fetch::Error::AddressNotAllowed { host } => {
                SearchError::Network(format!("Address not allowed (SSRF protection): {host}"))
            }
            servo_fetch::Error::Engine { url, .. } => {
                SearchError::Network(format!(
                    "Servo engine error for {}",
                    url.as_deref().unwrap_or("unknown URL")
                ))
            }
            servo_fetch::Error::Extract(_) => {
                SearchError::Parse(format!("Content extraction failed: {e}"))
            }
            _ => SearchError::Other(format!("servo-fetch error: {e}")),
        }
    }
}

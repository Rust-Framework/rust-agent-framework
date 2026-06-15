//! 核心类型定义：搜索结果、搜索配置、抓取配置等。

/// 搜索来源引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSource {
    /// DuckDuckGo Lite (`lite.duckduckgo.com`)
    DuckDuckGoLite,
    /// DuckDuckGo HTML (`html.duckduckgo.com`)
    DuckDuckGoHtml,
    /// DuckDuckGo Instant Answer API (`api.duckduckgo.com`)
    DuckDuckGoInstantAnswer,
    /// SearXNG 自建实例
    SearXNG,
}

/// 单条搜索结果。
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// 结果标题
    pub title: String,
    /// 结果 URL
    pub url: String,
    /// 摘要 / 片段
    pub snippet: String,
    /// 来源引擎
    pub source: SearchSource,
    /// 排名（1-based）
    pub rank: usize,
}

/// 搜索结果集合。
#[derive(Debug, Clone)]
pub struct SearchResults {
    /// 原始查询词
    pub query: String,
    /// 结果列表
    pub results: Vec<SearchResult>,
    /// 实际使用的搜索来源
    pub source: SearchSource,
}

impl SearchResults {
    /// 创建空结果集。
    pub fn empty(query: impl Into<String>, source: SearchSource) -> Self {
        Self {
            query: query.into(),
            results: Vec::new(),
            source,
        }
    }
}

/// 搜索配置。
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// 最大返回结果数（默认 10）
    pub max_results: usize,
    /// 超时时间（秒，默认 15）
    pub timeout_secs: u64,
    /// 最小请求间隔（毫秒，默认 1000）
    pub min_interval_ms: u64,
    /// HTTP/SOCKS5 代理地址
    pub proxy_url: Option<String>,
    /// SearXNG 实例地址（如 `https://searx.example.com`）
    pub searxng_url: Option<String>,
    /// 搜索语言（如 `zh-CN`、`en-US`）
    pub language: Option<String>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_results: 10,
            timeout_secs: 15,
            min_interval_ms: 1000,
            proxy_url: None,
            searxng_url: None,
            language: None,
        }
    }
}

impl SearchConfig {
    /// 创建新的 SearchConfig，max_results 自动 clamp 到 [1, 20]。
    pub fn new(max_results: usize) -> Self {
        Self {
            max_results: max_results.clamp(1, 20),
            ..Default::default()
        }
    }

    /// 设置代理。
    pub fn with_proxy(mut self, proxy_url: impl Into<String>) -> Self {
        self.proxy_url = Some(proxy_url.into());
        self
    }

    /// 设置语言。
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }
}

/// 网页抓取配置。
#[derive(Debug, Clone)]
pub struct FetchConfig {
    /// 超时时间（秒，默认 15）
    pub timeout_secs: u64,
    /// 最大内容长度（字节，默认 50KB）
    pub max_content_bytes: usize,
    /// HTTP/SOCKS5 代理地址
    pub proxy_url: Option<String>,
    /// 最小请求间隔（毫秒）
    pub min_interval_ms: u64,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 15,
            max_content_bytes: 50 * 1024,
            proxy_url: None,
            min_interval_ms: 1000,
        }
    }
}

/// 抓取到的网页内容。
#[derive(Debug, Clone)]
pub struct FetchedPage {
    /// 原始 URL
    pub url: String,
    /// 最终 URL（经过重定向后）
    pub final_url: String,
    /// 页面标题
    pub title: String,
    /// 正文内容（纯文本）
    pub content: String,
    /// 原始内容长度（字节）
    pub content_length: usize,
    /// 是否被截断
    pub truncated: bool,
    /// HTTP 状态码
    pub status_code: u16,
}

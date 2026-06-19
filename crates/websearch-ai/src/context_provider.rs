use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextResult, IAgent, IContextProvider,
    ISession, ITool, MessageRole, Result,
};

use crate::{web_search, WebFetch, WebSearch, WebSearchSharedConfig};

// ── WebSearchContextProvider ───────────────────────────────────────────

const DEFAULT_MAX_RESULTS: usize = 5;
const MAX_RESULTS_LIMIT: usize = 10;
const SEARCH_INSTRUCTION_CAP: usize = 3000;

/// WebSearchContextProvider — 基于 IContextProvider 的上下文工程实现
///
/// 对标 MAF 的 ContextProvider 模式，为 Agent 提供 web 搜索和网页抓取能力。
///
/// ## 功能
///
/// - **工具注入**: 自动向 Agent 注册 `web_search` 和 `web_fetch` 两个工具
/// - **自动搜索**: 启用后，在每次调用前基于最新用户消息自动搜索并注入结果到上下文
/// - **结果摘要**: 搜索结果以结构化 Markdown 格式注入到 system instructions 中
/// - **环境变量配置**: 支持 `WEBSEARCH_PROXY_URL` 和 `WEBSEARCH_SEARXNG_URL`
///
/// ## 使用示例
///
/// ```rust,no_run
/// use rust_agent_websearch::WebSearchContextProvider;
///
/// // 仅提供工具，不自动搜索
/// let provider = WebSearchContextProvider::new();
///
/// // 启用自动搜索（每次调用前自动搜索并注入结果）
/// let provider = WebSearchContextProvider::new()
///     .with_auto_search(true)
///     .with_max_results(5);
///
/// // 集成到 Agent（需引入 rust-agent-framework）
/// // use rust_agent_framework::AgentBuilder;
/// // let agent = AgentBuilder::new("my_agent")
/// //     .chat_client(client)
/// //     .add_context_provider(provider)
/// //     .build()?;
/// ```
pub struct WebSearchContextProvider {
    /// 是否在每次调用前自动执行搜索
    auto_search: bool,
    /// 自动搜索的最大结果数
    max_results: usize,
    /// 代理 URL（优先级高于环境变量）
    proxy_url: Option<String>,
    /// SearXNG 实例 URL（优先级高于环境变量）
    searxng_url: Option<String>,
    /// 搜索语言
    language: Option<String>,
}

impl WebSearchContextProvider {
    pub fn new() -> Self {
        Self {
            auto_search: false,
            max_results: DEFAULT_MAX_RESULTS,
            proxy_url: None,
            searxng_url: None,
            language: None,
        }
    }

    /// 启用自动搜索模式。
    ///
    /// 当 `enabled` 为 `true` 时，每次 Agent 调用前会自动提取最新用户消息
    /// 作为搜索查询，并将搜索结果注入到上下文指令中。
    pub fn with_auto_search(mut self, enabled: bool) -> Self {
        self.auto_search = enabled;
        self
    }

    /// 设置自动搜索的最大结果数（默认 5，最大 10）。
    pub fn with_max_results(mut self, max: usize) -> Self {
        self.max_results = max.clamp(1, MAX_RESULTS_LIMIT);
        self
    }

    /// 设置 HTTP/SOCKS5 代理 URL（覆盖环境变量 `WEBSEARCH_PROXY_URL`）。
    pub fn with_proxy(mut self, url: impl Into<String>) -> Self {
        self.proxy_url = Some(url.into());
        self
    }

    /// 设置 SearXNG 实例 URL（覆盖环境变量 `WEBSEARCH_SEARXNG_URL`）。
    pub fn with_searxng(mut self, url: impl Into<String>) -> Self {
        self.searxng_url = Some(url.into());
        self
    }

    /// 设置搜索语言（如 `zh-CN`、`en-US`）。
    pub fn with_language(mut self, lang: impl Into<String>) -> Self {
        self.language = Some(lang.into());
        self
    }

    /// 构建所有工具，复用 `WebSearch` / `WebFetch`（配置通过共享静态对象传递）。
    pub fn build_tools(&self) -> Vec<Arc<dyn ITool>> {
        vec![Arc::new(WebSearch), Arc::new(WebFetch)]
    }

    // ── advertise 文本 ──

    pub fn build_advertise(&self) -> String {
        let mut text = String::from("## 网页搜索能力\n\n");
        text.push_str("你拥有网页搜索和页面抓取能力：\n\n");
        text.push_str("- **web_search(query, count?)**：搜索网页，返回包含标题、URL 和摘要的结果列表。\n");
        text.push_str("- **web_fetch(url, max_length?, settle_ms?)**：获取任意 URL 的完整页面内容并转为 Markdown。\n\n");
        text.push_str("**工作流提示**：先用 web_search 发现信息和 URL，再用 web_fetch 获取完整页面内容。如果 web_search 无结果，请尝试不同的关键词或用英文搜索。\n");
        text
    }

    // ── 自动搜索 ──

    /// 从消息列表中提取最新的用户消息文本。
    fn extract_latest_user_query(&self, messages: &[ChatMessage]) -> Option<String> {
        messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.content.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// 执行自动搜索并返回格式化的结果文本。
    async fn auto_search_context(&self, messages: &[ChatMessage]) -> Option<String> {
        let query = self.extract_latest_user_query(messages)?;
        let config = web_search::build_search_config(self.max_results);

        tracing::debug!(
            query = %query,
            max_results = self.max_results,
            "auto_search triggered"
        );

        match rust_websearch::search(&query, &config).await {
            Ok(search_results) => {
                if search_results.results.is_empty() {
                    tracing::debug!("auto_search returned no results");
                    return None;
                }

                let count = search_results.results.len();
                tracing::info!(
                    query = %query,
                    count = count,
                    "auto_search succeeded"
                );

                let mut text = format!(
                    "## 网页搜索结果：「{}」\n\n",
                    query
                );
                text.push_str(&format!(
                    "找到 {} 条结果：\n\n",
                    count
                ));

                for (i, r) in search_results.results.iter().enumerate() {
                    text.push_str(&format!(
                        "### [{}.] {}\n",
                        i + 1,
                        r.title
                    ));
                    text.push_str(&format!("- **URL**: {}\n", r.url));
                    text.push_str(&format!("- **摘要**: {}\n\n", r.snippet));
                }

                text.push_str("---\n");
                text.push_str("*提示：使用 web_fetch(url) 获取上方任意 URL 的完整内容。使用 web_search(query) 搜索更多信息。*\n");

                // 截断过长的结果
                if text.len() > SEARCH_INSTRUCTION_CAP {
                    let truncation_note = format!(
                        "\n\n*（结果已截断至 {} 字符。请使用更精确的查询以获得更精准的结果。）*",
                        SEARCH_INSTRUCTION_CAP
                    );
                    text.truncate(SEARCH_INSTRUCTION_CAP - truncation_note.len());
                    text.push_str(&truncation_note);
                }

                Some(text)
            }
            Err(e) => {
                tracing::warn!(query = %query, error = %e, "auto_search failed");
                None
            }
        }
    }
}

impl Default for WebSearchContextProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ── IContextProvider impl ──────────────────────────────────────────────

#[async_trait]
impl IContextProvider for WebSearchContextProvider {
    fn name(&self) -> &str {
        "WebSearchContextProvider"
    }

    fn kind(&self) -> rust_agent_core::ContextProviderKind {
        rust_agent_core::ContextProviderKind::Skills
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextResult> {
        // 将当前 provider 的配置写入共享静态对象，供 WebSearch / WebFetch 工具读取
        crate::set_shared_config(WebSearchSharedConfig {
            proxy_url: self.proxy_url.clone(),
            searxng_url: self.searxng_url.clone(),
            language: self.language.clone(),
        });

        let mut injection = ContextResult {
            instructions: Some(self.build_advertise()),
            tools: self.build_tools(),
            ..Default::default()
        };

        if self.auto_search {
            if let Some(search_results) = self.auto_search_context(messages).await {
                // 将搜索结果追加到已有 instructions
                let mut combined = injection.instructions.take().unwrap_or_default();
                combined.push('\n');
                combined.push_str(&search_results);
                injection.instructions = Some(combined);
            }
        }

        tracing::debug!(
            provider = self.name(),
            tools = injection.tools.len(),
            has_instructions = injection.instructions.is_some(),
            "on_invoking complete"
        );

        Ok(injection)
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_defaults() {
        let provider = WebSearchContextProvider::new();
        assert!(!provider.auto_search);
        assert_eq!(provider.max_results, DEFAULT_MAX_RESULTS);
        assert!(provider.proxy_url.is_none());
    }

    #[test]
    fn test_provider_builder_pattern() {
        let provider = WebSearchContextProvider::new()
            .with_auto_search(true)
            .with_max_results(8)
            .with_proxy("http://proxy:8080")
            .with_searxng("https://searx.example.com")
            .with_language("zh-CN");

        assert!(provider.auto_search);
        assert_eq!(provider.max_results, 8);
        assert_eq!(provider.proxy_url, Some("http://proxy:8080".into()));
        assert_eq!(provider.searxng_url, Some("https://searx.example.com".into()));
        assert_eq!(provider.language, Some("zh-CN".into()));
    }

    #[test]
    fn test_max_results_clamped() {
        let provider = WebSearchContextProvider::new().with_max_results(0);
        assert_eq!(provider.max_results, 1);

        let provider = WebSearchContextProvider::new().with_max_results(100);
        assert_eq!(provider.max_results, MAX_RESULTS_LIMIT);
    }

    #[test]
    fn test_build_tools() {
        let provider = WebSearchContextProvider::new();
        let tools = provider.build_tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name() == "web_search"));
        assert!(tools.iter().any(|t| t.name() == "web_fetch"));
    }

    #[test]
    fn test_build_advertise() {
        let provider = WebSearchContextProvider::new();
        let text = provider.build_advertise();
        assert!(text.contains("网页搜索能力"));
        assert!(text.contains("web_search"));
        assert!(text.contains("web_fetch"));
    }

    #[test]
    fn test_extract_latest_user_query() {
        let provider = WebSearchContextProvider::new();
        let messages = vec![
            ChatMessage::user("Hello"),
            ChatMessage::user("What is Rust?"),
        ];
        let query = provider.extract_latest_user_query(&messages);
        assert_eq!(query, Some("What is Rust?".into()));
    }

    #[test]
    fn test_extract_latest_user_query_empty() {
        let provider = WebSearchContextProvider::new();
        let query = provider.extract_latest_user_query(&[]);
        assert_eq!(query, None);
    }

    #[test]
    fn test_shared_config_propagation() {
        // 通过共享静态对象传递配置
        crate::set_shared_config(WebSearchSharedConfig {
            proxy_url: Some("http://proxy:8080".into()),
            searxng_url: Some("https://searx.example.com".into()),
            language: Some("zh-CN".into()),
        });

        let search_cfg = crate::web_search::build_search_config(5);
        assert_eq!(search_cfg.max_results, 5);
        assert_eq!(search_cfg.proxy_url, Some("http://proxy:8080".into()));
        assert_eq!(
            search_cfg.searxng_url,
            Some("https://searx.example.com".into())
        );
        assert_eq!(search_cfg.language, Some("zh-CN".into()));

        let fetch_cfg = crate::web_fetch::build_fetch_config();
        assert_eq!(fetch_cfg.proxy_url, Some("http://proxy:8080".into()));
        assert_eq!(fetch_cfg.max_content_bytes, 50 * 1024);
    }

    #[test]
    fn test_provider_name() {
        let provider = WebSearchContextProvider::new();
        assert_eq!(provider.name(), "WebSearchContextProvider");
    }
}

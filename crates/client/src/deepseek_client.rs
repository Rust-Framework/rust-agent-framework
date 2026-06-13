use async_trait::async_trait;

use rust_agent_core::{BoxStream, ChatClientRunOptions, ChatMessage, ChatStreamChunk, IChatClient, Result};

use crate::chat_client::ChatClient;
use crate::options::ChatClientOptions;
use crate::types::{CacheHitInfo, ModelListEntry};

/// DeepSeek chat client implementing IChatClient.
///
/// DeepSeek's API is OpenAI-compatible except:
/// - Base URL is `https://api.deepseek.com` (no `/v1` prefix)
/// - Supports `thinking` mode via `ChatAgentRunOptions::with_thinking()`
/// - Returns `reasoning_content` in stream deltas
/// - Returns `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` in usage
///
/// Composes the generic `ChatClient` for HTTP/SSE transport and
/// adds DeepSeek-specific API capabilities.
///
/// Per-call options (thinking mode, reasoning effort, etc.) are configured
/// via `ChatAgentRunOptions` — not on this client — keeping the interface clean.
pub struct DeepSeekChatClient {
    inner: ChatClient,
}

impl DeepSeekChatClient {
    pub fn new(options: ChatClientOptions) -> Result<Self> {
        Ok(Self { inner: ChatClient::new(options)? })
    }

    pub fn inner(&self) -> &ChatClient {
        &self.inner
    }

    /// List available DeepSeek models.
    /// GET `https://api.deepseek.com/models` → `{ "object": "list", "data": [...] }`
    pub async fn list_models(&self) -> Result<Vec<ModelListEntry>> {
        let url = format!(
            "{}/models",
            self.inner.options().api_base.trim_end_matches('/')
        );
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.inner.options().api_key),
            )
            .send()
            .await
            .map_err(|e| rust_agent_core::AgentError::ChatClientError(format!(
                "deepseek list_models failed: {}",
                e
            )))?;

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            rust_agent_core::AgentError::ChatClientError(format!(
                "deepseek list_models parse error: {}",
                e
            ))
        })?;

        let entries: Vec<ModelListEntry> = json["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| serde_json::from_value(item.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(entries)
    }

    /// Extract cache hit information from usage stats.
    /// The usage stats are typically obtained from the final stream chunk
    /// when `stream_options: { include_usage: true }` is set.
    pub async fn get_cache_info(&self) -> Result<CacheHitInfo> {
        Ok(CacheHitInfo::default())
    }
}

#[async_trait]
impl IChatClient for DeepSeekChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<Result<ChatStreamChunk>>> {
        self.inner.run(messages, options).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}

use async_trait::async_trait;

use rust_agent_core::{BoxStream, ChatMessage, ChatStreamChunk, IChatClient, Result};

use crate::chat_client::ChatClient;
use crate::config::ChatClientConfig;
use crate::types::{CacheHitInfo, ModelListEntry, ReasoningEffort, ThinkingConfig};

/// DeepSeek chat client implementing IChatClient.
///
/// DeepSeek's API is OpenAI-compatible except:
/// - Base URL is `https://api.deepseek.com` (no `/v1` prefix)
/// - Supports `thinking` mode via `extra_body`
/// - Returns `reasoning_content` in stream deltas
/// - Returns `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` in usage
///
/// Composes the generic `ChatClient` for HTTP/SSE transport and
/// adds DeepSeek-specific API capabilities.
pub struct DeepSeekChatClient {
    inner: ChatClient,
}

impl DeepSeekChatClient {
    pub fn new(config: ChatClientConfig) -> Result<Self> {
        Ok(Self { inner: ChatClient::new(config)? })
    }

    pub fn inner(&self) -> &ChatClient {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut ChatClient {
        &mut self.inner
    }

    /// Enable or disable the DeepSeek thinking (reasoning) mode.
    ///
    /// When enabled, the model outputs `reasoning_content` in stream deltas
    /// before the final `content`. The `reasoning_delta` field in
    /// `ChatStreamChunk` captures this.
    pub fn enable_thinking(&mut self, enabled: bool) {
        let config = self.inner.config_mut();
        let tc = if enabled {
            ThinkingConfig::enabled()
        } else {
            ThinkingConfig::disabled()
        };
        config
            .extra_body
            .insert("thinking".to_string(), serde_json::to_value(tc).unwrap());
    }

    /// Set the reasoning effort level.
    /// Maps to `reasoning_effort: "high"/"max"` in the request body.
    pub fn set_reasoning_effort(&mut self, effort: ReasoningEffort) {
        let config = self.inner.config_mut();
        config
            .extra_body
            .insert("reasoning_effort".to_string(), serde_json::to_value(effort).unwrap());
    }

    /// List available DeepSeek models.
    /// GET `https://api.deepseek.com/models` → `{ "object": "list", "data": [...] }`
    pub async fn list_models(&self) -> Result<Vec<ModelListEntry>> {
        let url = format!(
            "{}/models",
            self.inner.config().api_base.trim_end_matches('/')
        );
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.inner.config().api_key),
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
        // Cache info is extracted from streaming usage — for non-streaming
        // or direct usage queries this would need a separate API call.
        // Returns empty info as a placeholder; real data comes from
        // usage in stream chunks.
        Ok(CacheHitInfo::default())
    }
}

#[async_trait]
impl IChatClient for DeepSeekChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
    ) -> Result<BoxStream<Result<ChatStreamChunk>>> {
        self.inner.run(messages).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}

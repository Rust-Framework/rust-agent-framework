use async_trait::async_trait;

use rust_agent_core::{BoxStream, ChatClientRunOptions, ChatMessage, ChatStreamChunk, IChatClient, Result};

use crate::chat_client::ChatClient;
use crate::options::ChatClientOptions;
use crate::types::{ModelListEntry, UsageStats};

/// OpenAI chat client implementing IChatClient.
///
/// Composes the generic `ChatClient` for HTTP/SSE transport and
/// adds OpenAI-specific API capabilities.
pub struct OpenAiChatClient {
    inner: ChatClient,
}

impl OpenAiChatClient {
    pub fn new(options: ChatClientOptions) -> Result<Self> {
        Ok(Self { inner: ChatClient::new(options)? })
    }

    pub fn inner(&self) -> &ChatClient {
        &self.inner
    }

    /// List available models.
    /// GET `{api_base}/models` → `{ "object": "list", "data": [...] }`
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
            .map_err(|e| rust_agent_core::AgentError::ChatClientError(format!("list_models failed: {}", e)))?;

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            rust_agent_core::AgentError::ChatClientError(format!("list_models parse error: {}", e))
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

    /// Get usage statistics (placeholder — OpenAI billing API).
    pub async fn get_usage(&self) -> Result<UsageStats> {
        Ok(UsageStats::default())
    }
}

#[async_trait]
impl IChatClient for OpenAiChatClient {
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

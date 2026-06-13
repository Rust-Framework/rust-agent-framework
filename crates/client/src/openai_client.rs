use async_trait::async_trait;

use rust_agent_core::{BoxStream, ChatMessage, ChatStreamChunk, IChatClient, Result};

use crate::chat_client::ChatClient;
use crate::config::ChatClientConfig;
use crate::types::{ModelListEntry, UsageStats};

/// OpenAI chat client implementing IChatClient.
///
/// Composes the generic `ChatClient` for HTTP/SSE transport and
/// adds OpenAI-specific API capabilities.
pub struct OpenAiChatClient {
    inner: ChatClient,
}

impl OpenAiChatClient {
    pub fn new(config: ChatClientConfig) -> Result<Self> {
        Ok(Self { inner: ChatClient::new(config)? })
    }

    pub fn inner(&self) -> &ChatClient {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut ChatClient {
        &mut self.inner
    }

    /// List available models.
    /// GET `{api_base}/models` → `{ "object": "list", "data": [...] }`
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
    /// In production this would call the OpenAI billing/usage endpoint.
    pub async fn get_usage(&self) -> Result<UsageStats> {
        Ok(UsageStats::default())
    }
}

#[async_trait]
impl IChatClient for OpenAiChatClient {
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

use async_trait::async_trait;

use rust_agent_core::{BoxStream, ChatMessage, ChatStreamChunk, IChatClient, Result};

use crate::config::ChatClientConfig;

/// OpenAI chat client implementing IChatClient.
///
/// Provider-leading naming convention following MAF's ADR-0021.
pub struct OpenAIChatClient {
    config: ChatClientConfig,
}

impl OpenAIChatClient {
    pub fn new(config: ChatClientConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ChatClientConfig {
        &self.config
    }
}

#[async_trait]
impl IChatClient for OpenAIChatClient {
    async fn run(&self, messages: &[ChatMessage]) -> Result<BoxStream<Result<ChatStreamChunk>>> {
        tracing::info!(
            "OpenAIChatClient: streaming from {} with model {}",
            self.config.api_base,
            self.config.model
        );

        // TODO: Actual HTTP streaming call to OpenAI API
        let last_msg = messages.last().map(|m| m.content.clone()).unwrap_or_default();
        let model = self.config.model.clone();

        let stream = futures_util::stream::once(async move {
            Ok(ChatStreamChunk {
                text_delta: Some(format!("[Echo from {}] {}", model, last_msg)),
                tool_call_delta: None,
            })
        });

        Ok(Box::pin(stream))
    }

    fn model_id(&self) -> &str {
        &self.config.model
    }
}

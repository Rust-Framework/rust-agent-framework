use async_trait::async_trait;
use std::time::Duration;

use rust_agent_core::{AgentError, BoxStream, ChatMessage, ChatStreamChunk, IChatClient, MessageRole, Result};

use crate::config::ChatClientConfig;
use crate::transport::SseStream;

/// Generic chat client that handles the HTTP transport and SSE streaming layer.
///
/// Works with any OpenAI-compatible API (OpenAI, DeepSeek, etc.).
/// Provider-specific extensions are implemented via wrapper types
/// (`OpenAiChatClient`, `DeepSeekChatClient`) that compose this struct.
pub struct ChatClient {
    http: reqwest::Client,
    config: ChatClientConfig,
}

impl ChatClient {
    pub fn new(config: ChatClientConfig) -> Result<Self> {
        let timeout = Duration::from_secs(config.timeout_secs.unwrap_or(60));
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| AgentError::ConfigError(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { http, config })
    }

    pub fn config(&self) -> &ChatClientConfig {
        &self.config
    }

    /// Mutable access to config — used by provider wrappers to modify `extra_body` etc.
    pub fn config_mut(&mut self) -> &mut ChatClientConfig {
        &mut self.config
    }

    /// Core streaming call: POST to `{api_base}/chat/completions`, parse SSE.
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<BoxStream<Result<ChatStreamChunk>>> {
        let url = format!("{}/chat/completions", self.config.api_base.trim_end_matches('/'));
        let body = self.build_request_body(messages);

        tracing::debug!("ChatClient request to {} with model {}", url, self.config.model);

        let mut req = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        // Inject provider-specific extra headers
        for (key, value) in &self.config.extra_headers {
            req = req.header(key, value);
        }

        let response = req.send().await.map_err(|e| {
            AgentError::ChatClientError(format!("HTTP request failed: {}", e))
        })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AgentError::ChatClientError(format!(
                "Chat API returned {}: {}",
                status, error_text
            )));
        }

        let byte_stream = response.bytes_stream();
        let sse = SseStream::new(byte_stream);
        Ok(Box::pin(sse))
    }

    /// Build the JSON request body for POST /chat/completions.
    fn build_request_body(&self, messages: &[ChatMessage]) -> serde_json::Value {
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let mut obj = serde_json::json!({
                    "role": match m.role {
                        MessageRole::System => "system",
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::Tool => "tool",
                    },
                    "content": m.content,
                });
                if let Some(name) = &m.name {
                    obj["name"] = serde_json::Value::String(name.clone());
                }
                obj
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": msgs,
            "stream": true,
            "stream_options": {
                "include_usage": true,
            },
        });

        if let Some(mt) = self.config.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(ref top_p) = self.config.top_p {
            body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(ref stop) = self.config.stop {
            body["stop"] = serde_json::json!(stop);
        }

        // Merge provider-specific extra_body fields at top level
        if let Some(obj) = body.as_object_mut() {
            for (key, value) in &self.config.extra_body {
                obj.insert(key.clone(), value.clone());
            }
        }

        body
    }
}

#[async_trait]
impl IChatClient for ChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
    ) -> Result<BoxStream<Result<ChatStreamChunk>>> {
        self.chat_stream(messages).await
    }

    fn model_id(&self) -> &str {
        &self.config.model
    }
}

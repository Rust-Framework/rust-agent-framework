//! Anthropic Messages API chat client.
//!
//! Docs: https://docs.anthropic.com/en/api/messages
//! - Endpoint: `POST /v1/messages`
//! - Auth: `x-api-key` + `anthropic-version: 2023-06-01`
//! - System prompt as top-level `system` field
//! - Tools use `input_schema`; tool results are `user` + `tool_result` blocks

use std::time::Duration;

use async_trait::async_trait;

use rust_agent_core::{
    AgentError, AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient,
    ModelMetadata, Result,
};

use crate::anthropic_messages::{convert_messages, convert_tools};
use crate::anthropic_stream::AnthropicSseStream;
use crate::options::ChatClientOptions;

pub const ANTHROPIC_DEFAULT_API_BASE: &str = "https://api.anthropic.com/v1";
pub const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Anthropic Messages API 聊天客户端。
pub struct AnthropicChatClient {
    http: reqwest::Client,
    options: ChatClientOptions,
}

impl AnthropicChatClient {
    pub fn new(mut options: ChatClientOptions) -> Result<Self> {
        if options.api_base.is_empty()
            || options.api_base == "https://api.openai.com/v1"
        {
            options.api_base = ANTHROPIC_DEFAULT_API_BASE.to_string();
        }
        if options.model_metadata.is_none() {
            options.model_metadata = Some(anthropic_model_metadata(&options.model));
        }
        let timeout = Duration::from_secs(options.timeout_secs.unwrap_or(60));
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| AgentError::ConfigError(format!("Failed to build HTTP client: {}", e)))?;
        Ok(Self { http, options })
    }

    pub fn options(&self) -> &ChatClientOptions {
        &self.options
    }

    fn build_request_body(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
    ) -> serde_json::Value {
        let (system, anthropic_messages) = convert_messages(messages);

        let max_tokens = run_options
            .max_tokens
            .or(self.options.max_tokens)
            .unwrap_or(4096);

        let mut body = serde_json::json!({
            "model": self.options.model,
            "max_tokens": max_tokens,
            "messages": anthropic_messages,
            "stream": true,
        });

        if let Some(system) = system {
            body["system"] = serde_json::Value::String(system);
        }

        let temperature = run_options.temperature.or(self.options.temperature);
        let top_p = run_options.top_p.or(self.options.top_p);
        let stop = run_options.stop.as_ref().or(self.options.stop.as_ref());

        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(ref top_p) = top_p {
            body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(ref stop) = stop {
            body["stop_sequences"] = serde_json::json!(stop);
        }

        if !run_options.tools.is_empty() {
            body["tools"] = serde_json::json!(convert_tools(&run_options.tools));
        }

        if let Some(obj) = body.as_object_mut() {
            for (key, value) in &run_options.extra_body {
                obj.insert(key.clone(), value.clone());
            }
        }

        body
    }

    async fn messages_stream(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let url = format!(
            "{}/messages",
            self.options.api_base.trim_end_matches('/')
        );
        let body = self.build_request_body(messages, run_options);

        tracing::debug!(
            "AnthropicChatClient request to {} with model {}",
            url,
            self.options.model
        );

        let mut req = self
            .http
            .post(&url)
            .header("x-api-key", &self.options.api_key)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("Content-Type", "application/json")
            .json(&body);

        for (key, value) in &self.options.extra_headers {
            req = req.header(key, value);
        }

        let response = req.send().await.map_err(|e| {
            AgentError::ChatClientError(format!("HTTP request failed: {}", e))
        })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AgentError::ChatClientError(format!(
                "Anthropic API returned {}: {}",
                status, error_text
            )));
        }

        let byte_stream = response.bytes_stream();
        let sse = AnthropicSseStream::new(byte_stream);
        Ok(Box::pin(sse))
    }
}

#[async_trait]
impl IChatClient for AnthropicChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        self.messages_stream(messages, &options).await
    }

    fn model_id(&self) -> &str {
        &self.options.model
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.options.model_metadata.as_ref()
    }
}

/// Anthropic 模型参考规格（常见 Claude 系列）。
pub fn anthropic_model_metadata(model_id: &str) -> ModelMetadata {
    let (context, max_output) = if model_id.contains("opus") {
        (200_000, 32_000)
    } else if model_id.contains("sonnet") || model_id.contains("haiku") {
        (200_000, 16_384)
    } else {
        (200_000, 8_192)
    };
    ModelMetadata::new(model_id, context, max_output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_messages_request_with_system_and_tools() {
        let client = AnthropicChatClient::new(ChatClientOptions {
            api_base: ANTHROPIC_DEFAULT_API_BASE.into(),
            api_key: "sk-test".into(),
            model: "claude-sonnet-4-20250514".into(),
            ..Default::default()
        })
        .unwrap();

        let messages = vec![
            ChatMessage::system("Be concise"),
            ChatMessage::user("hello"),
        ];
        let mut opts = ChatClientRunOptions::default();
        opts.tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "echo",
                "description": "echo",
                "parameters": { "type": "object", "properties": {} }
            }
        })];

        let body = client.build_request_body(&messages, &opts);
        assert_eq!(body["system"], "Be concise");
        assert_eq!(body["stream"], true);
        assert!(body.get("tools").is_some());
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }
}

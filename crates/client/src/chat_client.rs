use async_trait::async_trait;
use std::time::Duration;

use rust_agent_core::{
    AgentError, AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage,
    IChatClient, MessageRole, Result,
};

use crate::options::ChatClientOptions;
use crate::transport::SseStream;

/// Generic chat client that handles the HTTP transport and SSE streaming layer.
///
/// Works with any OpenAI-compatible API (OpenAI, DeepSeek, etc.).
/// Provider-specific extensions are implemented via wrapper types
/// (`OpenAiChatClient`, `DeepSeekChatClient`) that compose this struct.
pub struct ChatClient {
    http: reqwest::Client,
    options: ChatClientOptions,
}

impl ChatClient {
    pub fn new(options: ChatClientOptions) -> Result<Self> {
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

    /// Core streaming call: POST to `{api_base}/chat/completions`, parse SSE.
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let url = format!(
            "{}/chat/completions",
            self.options.api_base.trim_end_matches('/')
        );
        let body = self.build_request_body(messages, run_options);

        tracing::debug!(
            "ChatClient request to {} with model {}",
            url,
            self.options.model
        );

        let mut req = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.options.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        // Inject provider-specific extra headers
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
                "Chat API returned {}: {}",
                status, error_text
            )));
        }

        let byte_stream = response.bytes_stream();
        let sse = SseStream::new(byte_stream);
        Ok(Box::pin(sse))
    }

    /// Build the JSON request body for POST /chat/completions.
    ///
    /// Per-call `run_options` override the client's defaults.
    fn build_request_body(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
    ) -> serde_json::Value {
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
                // Serialize tool_calls for assistant messages
                if let Some(ref tool_calls) = m.tool_calls {
                    let tc_json: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        })
                        .collect();
                    obj["tool_calls"] = serde_json::Value::Array(tc_json);
                }
                // Include tool_call_id for tool messages
                if let Some(ref tool_call_id) = m.tool_call_id {
                    obj["tool_call_id"] =
                        serde_json::Value::String(tool_call_id.clone());
                }
                obj
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.options.model,
            "messages": msgs,
            "stream": true,
            "stream_options": {
                "include_usage": true,
            },
        });

        // Per-call overrides take precedence; fall back to client defaults
        let max_tokens = run_options.max_tokens.or(self.options.max_tokens);
        let temperature = run_options.temperature.or(self.options.temperature);
        let top_p = run_options.top_p.or(self.options.top_p);
        let stop = run_options.stop.as_ref().or(self.options.stop.as_ref());

        if let Some(mt) = max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(ref top_p) = top_p {
            body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(ref stop) = stop {
            body["stop"] = serde_json::json!(stop);
        }

        // Merge per-call extra_body fields at top level
        if let Some(obj) = body.as_object_mut() {
            for (key, value) in &run_options.extra_body {
                obj.insert(key.clone(), value.clone());
            }
        }

        // Include tool definitions if provided
        if !run_options.tools.is_empty() {
            body["tools"] = serde_json::json!(run_options.tools);
        }

        body
    }
}

#[async_trait]
impl IChatClient for ChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        self.chat_stream(messages, &options).await
    }

    fn model_id(&self) -> &str {
        &self.options.model
    }
}

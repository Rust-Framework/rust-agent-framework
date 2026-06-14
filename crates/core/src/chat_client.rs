use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{AgentResponseUpdate, BoxStream, ChatMessage, Result};

/// Per-call run options for `IChatClient::run()`, following MAF's pattern.
///
/// Overrides the client's defaults for a single call.
/// All fields are `Option` — `None` means "use the client's default".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatClientRunOptions {
    /// Override max_tokens for this call.
    pub max_tokens: Option<u32>,
    /// Override temperature for this call.
    pub temperature: Option<f32>,
    /// Override top_p for this call.
    pub top_p: Option<f32>,
    /// Override stop sequences for this call.
    pub stop: Option<Vec<String>>,
    /// Extra JSON fields merged into the request body top-level
    /// for this call only (e.g. `{"thinking": {"type": "enabled"}}`).
    pub extra_body: HashMap<String, serde_json::Value>,
    /// Tool definitions in OpenAI function-calling format.
    /// Each entry is a JSON object like:
    /// ```json
    /// { "type": "function", "function": { "name": "...", "description": "...", "parameters": {...} } }
    /// ```
    pub tools: Vec<serde_json::Value>,
    /// Allow parallel tool calls. When `Some(true)`, the LLM may emit multiple
    /// tool calls in a single response. When `Some(false)`, tool calls are
    /// serialized. `None` means use the provider default (typically enabled).
    /// Maps to OpenAI's `parallel_tool_calls` parameter.
    pub parallel_tool_calls: Option<bool>,
}

impl ChatClientRunOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_extra_body(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    pub fn with_tools(mut self, tools: Vec<serde_json::Value>) -> Self {
        self.tools = tools;
        self
    }
}

/// Chat client interface following MAF's ChatClient abstraction.
///
/// A thin wrapper over LLM provider APIs.
/// Only streaming output is supported.
#[async_trait]
pub trait IChatClient: Send + Sync {
    /// Run chat completion and produce a stream of update deltas.
    ///
    /// `options` allows per-call overrides (temperature, extra_body, etc.)
    /// without mutating the client's persistent configuration.
    /// Pass `Default::default()` for standard behaviour.
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>>;

    /// The model identifier used by this client.
    fn model_id(&self) -> &str;
}

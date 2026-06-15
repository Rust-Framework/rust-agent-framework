use std::collections::HashMap;

use rust_agent_core::ModelMetadata;
use serde::{Deserialize, Serialize};

/// Construction options for a chat client, following MAF's provider-leading naming.
///
/// These are set once at client creation time and remain static.
/// Per-call overrides (temperature, extra_body, etc.) belong in
/// `ChatClientRunOptions` — not here.
///
/// `api_base` stores the full base URL (e.g. `https://api.openai.com/v1` or
/// `https://api.deepseek.com`). It is NOT suffixed with `/v1` internally —
/// each provider has its own URL path convention (OpenAI uses `/v1/...`,
/// DeepSeek does not).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatClientOptions {
    pub api_base: String,
    /// API key — skipped during serialization to prevent accidental leakage.
    #[serde(skip)]
    pub api_key: String,
    pub model: String,
    /// Default max_tokens — can be overridden per-call via `ChatClientRunOptions`.
    pub max_tokens: Option<u32>,
    /// Default temperature — can be overridden per-call via `ChatClientRunOptions`.
    pub temperature: Option<f32>,
    /// Default top_p — can be overridden per-call via `ChatClientRunOptions`.
    pub top_p: Option<f32>,
    /// Default stop sequences — can be overridden per-call via `ChatClientRunOptions`.
    pub stop: Option<Vec<String>>,
    /// Extra HTTP headers merged into every request (e.g. `OpenAI-Organization`).
    #[serde(skip)]
    pub extra_headers: HashMap<String, String>,
    /// Request timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Model metadata describing capability boundaries (context window, max output).
    /// When `None`, the framework cannot enforce token limits automatically.
    #[serde(skip)]
    pub model_metadata: Option<ModelMetadata>,
}

impl std::fmt::Display for ChatClientOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ChatClientOptions {{ api_base: {}, model: {}, api_key: *** }}",
            self.api_base, self.model
        )
    }
}

impl Default for ChatClientOptions {
    fn default() -> Self {
        Self {
            api_base: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4".to_string(),
            max_tokens: None,
            temperature: None,
            top_p: None,
            stop: None,
            extra_headers: HashMap::new(),
            timeout_secs: Some(60),
            model_metadata: None,
        }
    }
}

impl ChatClientOptions {
    /// Create an OpenAI-flavoured options with the standard `/v1` base.
    pub fn openai(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_base: "https://api.openai.com/v1".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }

    /// Create a DeepSeek-flavoured options (base URL has **no** `/v1` prefix).
    pub fn deepseek(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_base: "https://api.deepseek.com".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }
}

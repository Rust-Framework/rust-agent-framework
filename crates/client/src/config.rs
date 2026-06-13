use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration for a chat client, following MAF's provider-leading naming.
///
/// `api_base` stores the full base URL (e.g. `https://api.openai.com/v1` or
/// `https://api.deepseek.com`). It is NOT suffixed with `/v1` internally —
/// each provider has its own URL path convention (OpenAI uses `/v1/...`,
/// DeepSeek does not).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatClientConfig {
    pub api_base: String,
    /// API key — skipped during serialization to prevent accidental leakage.
    #[serde(skip)]
    pub api_key: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    /// Extra HTTP headers merged into every request (e.g. `OpenAI-Organization`).
    #[serde(skip)]
    pub extra_headers: HashMap<String, String>,
    /// Extra JSON fields merged into the chat completion request body top-level.
    /// DeepSeek example: `{"thinking": {"type": "enabled"}}`.
    #[serde(skip)]
    pub extra_body: HashMap<String, serde_json::Value>,
    /// Request timeout in seconds.
    pub timeout_secs: Option<u64>,
}

impl std::fmt::Display for ChatClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ChatClientConfig {{ api_base: {}, model: {}, api_key: *** }}",
            self.api_base, self.model
        )
    }
}

impl Default for ChatClientConfig {
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
            extra_body: HashMap::new(),
            timeout_secs: Some(60),
        }
    }
}

impl ChatClientConfig {
    /// Create an OpenAI-flavoured config with the standard `/v1` base.
    pub fn openai(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_base: "https://api.openai.com/v1".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }

    /// Create a DeepSeek-flavoured config (base URL has **no** `/v1` prefix).
    pub fn deepseek(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_base: "https://api.deepseek.com".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }
}

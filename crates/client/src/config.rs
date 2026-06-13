use serde::{Deserialize, Serialize};

/// Configuration for a chat client, following MAF's provider-leading naming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatClientConfig {
    pub api_base: String,
    /// API key — skipped during serialization to prevent accidental leakage.
    #[serde(skip)]
    pub api_key: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
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
        }
    }
}

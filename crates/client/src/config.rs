use serde::{Deserialize, Serialize};

/// Configuration for a chat client, following MAF's provider-leading naming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatClientConfig {
    pub api_base: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
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

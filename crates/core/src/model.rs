use serde::{Deserialize, Serialize};

/// Model metadata and capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_tools: bool,
    pub supports_streaming: bool,
}

impl ModelInfo {
    pub fn openai_gpt4() -> Self {
        Self {
            id: "gpt-4".to_string(),
            provider: "openai".to_string(),
            context_window: 128_000,
            max_output_tokens: 4_096,
            supports_tools: true,
            supports_streaming: true,
        }
    }

    pub fn openai_gpt35_turbo() -> Self {
        Self {
            id: "gpt-3.5-turbo".to_string(),
            provider: "openai".to_string(),
            context_window: 16_385,
            max_output_tokens: 4_096,
            supports_tools: true,
            supports_streaming: true,
        }
    }
}

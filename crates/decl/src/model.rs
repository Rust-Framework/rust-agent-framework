use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::connection::Connection;

/// Model configuration for an AI agent.
/// Aligns with MAF AgentSchema v1.0 `Model`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    /// Model identifier (e.g., "gpt-4o", "deepseek-chat").
    pub id: String,
    /// Provider name (e.g., "openai", "azure", "anthropic").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// API type for the model.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "apiType")]
    pub api_type: Option<ApiType>,
    /// Connection configuration for authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<Connection>,
    /// Generation options (temperature, max tokens, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<ModelOptions>,
}

/// API type discriminator for the model endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiType {
    /// Standard chat completions API.
    Chat,
    /// Azure OpenAI Responses API.
    Responses,
}

/// Options controlling generation behavior.
/// Aligns with MAF AgentSchema v1.0 `ModelOptions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOptions {
    /// Kind discriminator for ModelOptions.
    pub kind: String,
    /// Penalty for token frequency (-2.0 to 2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// Maximum tokens in the output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    /// Penalty for token presence (-2.0 to 2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// Random seed for deterministic output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,
    /// Sampling temperature (0.0 to 2.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-K sampling value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    /// Top-P (nucleus) sampling value (0.0 to 1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Stop sequences that halt generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Whether to allow multiple tool calls per response.
    #[serde(default)]
    pub allow_multiple_tool_calls: Option<bool>,
    /// Additional custom properties for forward compatibility.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Model {
    /// Create a model with just an ID.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider: None,
            api_type: None,
            connection: None,
            options: None,
        }
    }

    /// Set the provider.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Set the connection.
    pub fn with_connection(mut self, connection: Connection) -> Self {
        self.connection = Some(connection);
        self
    }

    /// Set model options.
    pub fn with_options(mut self, options: ModelOptions) -> Self {
        self.options = Some(options);
        self
    }
}

impl ModelOptions {
    /// Create model options with a kind tag.
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            frequency_penalty: None,
            max_output_tokens: None,
            presence_penalty: None,
            seed: None,
            temperature: None,
            top_k: None,
            top_p: None,
            stop_sequences: None,
            allow_multiple_tool_calls: None,
            extra: HashMap::new(),
        }
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// Set max output tokens.
    pub fn with_max_output_tokens(mut self, tokens: i32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    /// Set top-p.
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }
}

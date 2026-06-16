use std::collections::HashMap;

use rust_agent_core::AgentRunOptions;
use serde::{Deserialize, Serialize};

use crate::error::{DeclError, Result};

// ── Model Configuration ──

/// Model provider and connection configuration for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Provider name: `"openai"`, `"deepseek"`, or `"custom"`.
    pub provider: String,
    /// Model name, e.g. `"gpt-4o"`, `"deepseek-chat"`.
    pub model: String,
    /// API key. Supports `$ENV_VAR` syntax to read from environment.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Optional API base URL override.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Default temperature for chat requests.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Default max_tokens for chat requests.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Extra HTTP headers merged into every request.
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    /// Arbitrary extra configuration forwarded to the provider.
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl ModelConfig {
    /// Resolve the API key. If the value starts with `$`, treat it as an
    /// environment variable name and read its value.
    pub fn resolve_api_key(&self) -> Result<String> {
        match &self.api_key {
            Some(key) if key.starts_with('$') => {
                let var_name = &key[1..];
                std::env::var(var_name).map_err(|_| {
                    DeclError::Resolution(format!(
                        "Environment variable '{}' not set (referenced by api_key)",
                        var_name
                    ))
                })
            }
            Some(key) => Ok(key.clone()),
            None => Err(DeclError::Missing(
                "api_key is required in model config".into(),
            )),
        }
    }
}

// ── Tool References ──

/// Reference to a tool that should be registered with the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolRef {
    /// A built-in framework tool.
    Builtin {
        /// Tool name: `"read_file"`, `"write_file"`, `"web_search"`, etc.
        name: String,
        /// Optional per-instance configuration (currently unused by builtins).
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
    /// A Rhai script tool.
    Rhai {
        /// Display name for the tool.
        name: String,
        /// Human-readable description.
        description: String,
        /// Path to the Rhai script file.
        script_path: String,
        /// JSON Schema describing the tool's parameters.
        #[serde(default)]
        parameters: serde_json::Value,
    },
    /// A custom tool registered via factory.
    Custom {
        /// Tool name used for factory lookup.
        name: String,
        /// Arbitrary configuration forwarded to the factory.
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
    },
}

// ── Context Provider Declarations ──

/// Declaration for a context provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextProviderDecl {
    /// In-memory history provider (built into AgentBuilder by default).
    InMemoryHistory,
    /// Skills provider — 按名称引用技能，架构从 skill_directories 自动查找并注册。
    Skills {
        /// 技能名称列表。
        names: Vec<String>,
    },
}

// ── Compression / Token Counter ──

/// Declaration for a compression strategy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompressionDecl {
    /// Sliding window compression.
    SlidingWindow { window_size: Option<usize> },
    /// Token budget compression.
    TokenBudget { budget: Option<usize> },
}

/// Declaration for a token counter.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TokenCounterDecl {
    /// Estimate counter (approximate token counting).
    Estimate,
}

// ── Agent Declaration ──

fn default_max_tool_rounds() -> usize {
    10
}

/// Complete declarative definition of an agent.
///
/// Follows the **Agent Declaration Protocol** — an open, format-agnostic
/// data schema for defining LLM-powered agents. Compatible with the
/// [OpenAI Chat Completions API](https://platform.openai.com/docs/api-reference/chat)
/// function-calling conventions and [JSON Schema](https://json-schema.org/)
/// for tool parameter definitions.
///
/// ## Protocol alignment
///
/// | Concept | Standard |
/// |---------|----------|
/// | Tool definitions | OpenAI Function Calling (`name` + `description` + `parameters` as JSON Schema) |
/// | Model config | OpenAI-compatible provider settings (`provider` + `model` + `api_key`) |
/// | Message format | OpenAI Chat Completions (`system`/`user`/`assistant`/`tool` roles) |
/// | Parameter schema | JSON Schema Draft-07 |
/// | Serialization | JSON / YAML / TOML |
///
/// Mirrors every capability of `AgentBuilder`, expressed as serializable data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDecl {
    /// Protocol version. Reserved for future compatibility.
    /// Current version: `"1.0"`.
    #[serde(default = "default_protocol_version")]
    pub version: String,
    /// URI of the JSON Schema for this declaration format.
    /// Example: `"https://agent-decl.dev/schemas/agent-decl-1.0.json"`
    #[serde(default, rename = "$schema", skip_serializing_if = "String::is_empty")]
    pub schema: String,
    /// Unique agent identifier.
    pub id: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// System instructions for the agent.
    #[serde(default)]
    pub instructions: String,
    /// Model configuration (required).
    pub model: ModelConfig,
    /// Tools to register.
    #[serde(default)]
    pub tools: Vec<ToolRef>,
    /// Context providers to attach.
    #[serde(default)]
    pub context_providers: Vec<ContextProviderDecl>,
    /// Skill directory search paths. Used when `Skills` context provider
    /// references skills by name — the resolver scans these directories.
    #[serde(default)]
    pub skill_directories: Vec<String>,
    /// Arbitrary key-value properties.
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    /// Maximum tool-calling rounds before forced stop.
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: usize,
    /// Compression strategy (optional).
    #[serde(default)]
    pub compression: Option<CompressionDecl>,
    /// Token counter (optional).
    #[serde(default)]
    pub token_counter: Option<TokenCounterDecl>,
    /// Per-run option overrides.
    #[serde(default)]
    pub run_options: Option<AgentRunOptions>,
    /// Nested sub-agent declarations (resolved recursively).
    #[serde(default)]
    pub sub_agents: Vec<AgentDecl>,
}

fn default_protocol_version() -> String {
    "1.0".into()
}

impl AgentDecl {
    // ── JSON ──

    /// Parse an `AgentDecl` from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Load an `AgentDecl` from a JSON file.
    pub fn from_json_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json_str(&content)
    }

    /// Serialize to a JSON string.
    pub fn to_json_string(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Serialize to a pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    // ── YAML ──

    /// Parse an `AgentDecl` from a YAML string.
    #[cfg(feature = "yaml")]
    pub fn from_yaml_str(s: &str) -> Result<Self> {
        Ok(serde_yaml::from_str(s)?)
    }

    /// Load an `AgentDecl` from a YAML file.
    #[cfg(feature = "yaml")]
    pub fn from_yaml_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml_str(&content)
    }

    /// Serialize to a YAML string.
    #[cfg(feature = "yaml")]
    pub fn to_yaml_string(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }

    // ── TOML ──

    /// Parse an `AgentDecl` from a TOML string.
    #[cfg(feature = "toml")]
    pub fn from_toml_str(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }

    /// Load an `AgentDecl` from a TOML file.
    #[cfg(feature = "toml")]
    pub fn from_toml_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    /// Serialize to a TOML string.
    #[cfg(feature = "toml")]
    pub fn to_toml_string(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }
}

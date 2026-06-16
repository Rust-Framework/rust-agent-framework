//! Host configuration — multi-layered config via figment + clap.

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

/// Top-level host configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    /// Transport mode: "stdio" or "ws".
    #[serde(default = "default_mode")]
    pub mode: TransportMode,
    /// WebSocket bind address (used only in Ws mode).
    #[serde(default = "default_ws_bind")]
    pub ws_bind: String,
    /// LLM provider configuration.
    #[serde(default)]
    pub provider: ProviderConfig,
    /// Built-in agent presets to enable.
    #[serde(default)]
    pub agents: AgentPresetsConfig,
    /// Directory for declarative agent files (JSON/YAML/TOML).
    #[serde(default)]
    pub agents_dir: Option<String>,
}

fn default_mode() -> TransportMode { TransportMode::Stdio }
fn default_ws_bind() -> String { "127.0.0.1:9876".into() }

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    #[value(name = "stdio")]
    Stdio,
    #[value(name = "ws")]
    Ws,
}

impl std::fmt::Display for TransportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportMode::Stdio => write!(f, "stdio"),
            TransportMode::Ws => write!(f, "ws"),
        }
    }
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider name: "deepseek", "openai", "custom".
    #[serde(default = "default_provider_name")]
    pub provider: String,
    /// Model name, e.g. "deepseek-v4-flash".
    #[serde(default = "default_model")]
    pub model: String,
    /// API key. Supports $ENV_VAR syntax.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Optional API base URL override.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Default temperature.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Default max_tokens.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

fn default_provider_name() -> String { "deepseek".into() }
fn default_model() -> String { "deepseek-v4-flash".into() }

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: default_provider_name(),
            model: default_model(),
            api_key: None,
            base_url: None,
            temperature: None,
            max_tokens: None,
        }
    }
}

impl ProviderConfig {
    /// Resolve the API key, supporting $ENV_VAR syntax.
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key.as_ref().and_then(|key| {
            if let Some(var_name) = key.strip_prefix('$') {
                std::env::var(var_name).ok()
            } else {
                Some(key.clone())
            }
        })
    }
}

/// Built-in agent presets to enable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentPresetsConfig {
    /// Enable the coding agent.
    #[serde(default = "default_true")]
    pub coding: bool,
    /// Enable the general agent.
    #[serde(default = "default_true")]
    pub general: bool,
    /// Enable the analysis agent.
    #[serde(default = "default_true")]
    pub analysis: bool,
}

fn default_true() -> bool { true }

/// CLI arguments (clap).
#[derive(Debug, Parser, Serialize)]
#[command(name = "rust-agent-host", about = "ACP server for the Rust Agent Framework")]
pub struct CliArgs {
    /// Transport mode: stdio or ws
    #[arg(long, value_enum)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<TransportMode>,
    /// WebSocket bind address (ws mode only)
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    /// Path to TOML config file
    #[arg(long)]
    #[serde(skip)]
    pub config: Option<String>,
    /// Directory for declarative agent files
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents_dir: Option<String>,
    /// LLM provider name
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// LLM model name
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// API key (or $ENV_VAR)
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// Load configuration from layered sources:
/// 1. host.toml (or --config path)
/// 2. RAF_ environment variables
/// 3. CLI arguments (highest priority)
pub fn load_config() -> anyhow::Result<HostConfig> {
    use figment::{
        Figment,
        providers::{Env, Format, Serialized, Toml},
    };

    let cli = CliArgs::parse();

    // Build figment with layered providers
    let mut fig = Figment::new();

    // Layer 1: TOML config file
    let config_path = cli.config.as_deref().unwrap_or("host.toml");
    if std::path::Path::new(config_path).exists() {
        fig = fig.merge(Toml::file(config_path));
    }

    // Layer 2: Environment variables (RAF_ prefix)
    fig = fig.merge(Env::prefixed("RAF_").split("__"));

    // Layer 3: CLI defaults (serialize cli args)
    fig = fig.merge(Serialized::defaults(&cli));

    let config: HostConfig = fig.extract()?;
    Ok(config)
}

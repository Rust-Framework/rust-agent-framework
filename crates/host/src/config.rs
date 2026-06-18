//! Host configuration — multi-layered config via figment + clap.

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

/// 顶层主机配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    /// 传输模式："stdio" 或 "ws"。
    #[serde(default = "default_mode")]
    pub mode: TransportMode,
    /// WebSocket 绑定地址（仅 Ws 模式下使用）。
    #[serde(default = "default_ws_bind")]
    pub ws_bind: String,
    /// LLM 提供商配置。
    #[serde(default)]
    pub provider: ProviderConfig,
    /// 要启用的内置 Agent 预设。
    #[serde(default)]
    pub agents: AgentPresetsConfig,
    /// 声明式 Agent 文件（JSON/YAML/TOML）的目录。
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

/// LLM 提供商配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// 提供商名称："deepseek"、"openai"、"custom"。
    #[serde(default = "default_provider_name")]
    pub provider: String,
    /// 模型名称，例如 "deepseek-v4-flash"。
    #[serde(default = "default_model")]
    pub model: String,
    /// API 密钥，支持 $ENV_VAR 语法。
    #[serde(default)]
    pub api_key: Option<String>,
    /// 可选的 API base URL 覆盖。
    #[serde(default)]
    pub base_url: Option<String>,
    /// 默认 temperature。
    #[serde(default)]
    pub temperature: Option<f32>,
    /// 默认 max_tokens。
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
    /// 解析 API 密钥，支持 $ENV_VAR 语法。
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

/// 要启用的内置 Agent 预设。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentPresetsConfig {
    /// 启用编码 Agent。
    #[serde(default = "default_true")]
    pub coding: bool,
    /// 启用通用 Agent。
    #[serde(default = "default_true")]
    pub general: bool,
    /// 启用分析 Agent。
    #[serde(default = "default_true")]
    pub analysis: bool,
}

fn default_true() -> bool { true }

/// CLI 参数（clap）。
#[derive(Debug, Parser, Serialize)]
#[command(name = "rust-agent-host", about = "ACP server for the Rust Agent Framework")]
pub struct CliArgs {
    /// 传输模式：stdio 或 ws
    #[arg(long, value_enum)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<TransportMode>,
    /// WebSocket 绑定地址（仅 ws 模式）
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    /// TOML 配置文件路径
    #[arg(long)]
    #[serde(skip)]
    pub config: Option<String>,
    /// 声明式 Agent 文件目录
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents_dir: Option<String>,
    /// LLM 提供商名称
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// LLM 模型名称
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// API 密钥（或 $ENV_VAR）
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

/// 从分层源加载配置：
/// 1. host.toml（或 --config 路径）
/// 2. RAF_ 环境变量
/// 3. CLI 参数（最高优先级）
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

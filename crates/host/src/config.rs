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
    /// 6 阶段开发流水线配置（rust-agent-coding）。
    #[serde(default)]
    pub dev_pipeline: DevPipelineConfig,
    /// 声明式 Agent 文件（JSON/YAML/TOML）的目录。
    #[serde(default)]
    pub agents_dir: Option<String>,
    /// 工作区根目录（供 Agent 文件工具使用）。默认为当前目录。
    #[serde(default = "default_workspace_root")]
    pub workspace_root: String,
}

fn default_mode() -> TransportMode { TransportMode::Stdio }
fn default_ws_bind() -> String { "127.0.0.1:9876".into() }
fn default_workspace_root() -> String { ".".into() }

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
    /// 提供商名称："openai"、"custom"。
    #[serde(default = "default_provider_name")]
    pub provider: String,
    /// 模型名称，例如 "agnes-2.0-flash"。
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
    /// 默认 max_tokens（输出上限）。
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// 模型上下文窗口大小（token 数）。用于自动上下文压缩。
    /// 例如 GPT-4o 为 128000。
    #[serde(default = "default_context_window")]
    pub context_window_tokens: usize,
    /// 模型最大输出 token 数。用于计算输入预算。
    /// 例如 GPT-4o 为 16384。
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: usize,
}

fn default_provider_name() -> String { "openai".into() }
fn default_model() -> String { "agnes-2.0-flash".into() }
fn default_context_window() -> usize { 128_000 }
fn default_max_output_tokens() -> usize { 8_192 }

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: default_provider_name(),
            model: default_model(),
            api_key: None,
            base_url: None,
            temperature: None,
            max_tokens: None,
            context_window_tokens: default_context_window(),
            max_output_tokens: default_max_output_tokens(),
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

/// 6 阶段开发流水线配置（rust-agent-coding 集成）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevPipelineConfig {
    /// 是否启用开发流水线 Agent（`dev-pipeline`）。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Agent ID（注册到 Registry 中的标识）。
    #[serde(default = "default_dev_pipeline_id")]
    pub agent_id: String,
    /// 反馈循环最大迭代次数（对应 `LoopConfig`）。
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

fn default_dev_pipeline_id() -> String { "dev-pipeline".into() }
fn default_max_iterations() -> u32 { 3 }

impl Default for DevPipelineConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_id: default_dev_pipeline_id(),
            max_iterations: default_max_iterations(),
        }
    }
}

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
    /// 工作区根目录（供 Agent 文件工具使用）
    #[arg(long)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    /// 禁用开发流水线 Agent
    #[arg(long)]
    #[serde(skip)]
    pub no_dev_pipeline: bool,
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

    let mut config: HostConfig = fig.extract()?;

    // Handle --no-dev-pipeline flag
    if cli.no_dev_pipeline {
        config.dev_pipeline.enabled = false;
    }

    Ok(config)
}

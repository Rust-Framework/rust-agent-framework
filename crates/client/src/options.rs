use std::collections::HashMap;

use rust_agent_core::ModelMetadata;
use serde::{Deserialize, Serialize};

/// 聊天客户端的构建选项，遵循 MAF 的提供商优先命名约定。
///
/// 这些选项在客户端创建时设置一次，之后保持静态。
/// 每次调用的覆盖项（temperature、extra_body 等）应放在 `ChatClientRunOptions` 中，而非此处。
///
/// `api_base` 存储完整的基础 URL（例如 `https://api.openai.com/v1` 或 `https://api.deepseek.com`）。
/// 内部不会自动添加 `/v1` 后缀——每个提供商有各自的 URL 路径约定（OpenAI 使用 `/v1/...`，DeepSeek 不使用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatClientOptions {
    pub api_base: String,
    /// API 密钥——序列化时跳过，防止意外泄露。
    #[serde(skip)]
    pub api_key: String,
    pub model: String,
    /// 默认 max_tokens——可通过 `ChatClientRunOptions` 每次调用时覆盖。
    pub max_tokens: Option<u32>,
    /// 默认 temperature——可通过 `ChatClientRunOptions` 每次调用时覆盖。
    pub temperature: Option<f32>,
    /// 默认 top_p——可通过 `ChatClientRunOptions` 每次调用时覆盖。
    pub top_p: Option<f32>,
    /// 默认 stop 序列——可通过 `ChatClientRunOptions` 每次调用时覆盖。
    pub stop: Option<Vec<String>>,
    /// 合并到每个请求中的额外 HTTP 头（例如 `OpenAI-Organization`）。
    #[serde(skip)]
    pub extra_headers: HashMap<String, String>,
    /// 请求超时时间（秒）。
    pub timeout_secs: Option<u64>,
    /// 流式响应是否在 `stream_options` 中请求 `include_usage`（部分兼容网关不支持）。
    pub stream_include_usage: bool,
    /// 当为 `None` 时，框架无法自动强制 token 限制。
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
            stream_include_usage: true,
            model_metadata: None,
        }
    }
}

impl ChatClientOptions {
    /// 创建 OpenAI 风格的选项，使用标准的 `/v1` 基础 URL。
    pub fn openai(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_base: "https://api.openai.com/v1".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }

    /// 创建 DeepSeek 风格的选项（基础 URL **没有** `/v1` 前缀）。
    pub fn deepseek(model: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            api_base: "https://api.deepseek.com".into(),
            api_key: api_key.into(),
            model: model.into(),
            ..Default::default()
        }
    }
}

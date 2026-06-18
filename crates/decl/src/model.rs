use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::connection::Connection;

/// AI Agent 的模型配置，与 MAF AgentSchema v1.0 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    /// 模型标识符（例如 "gpt-4o"、"deepseek-chat"）。
    pub id: String,
    /// 提供商名称（例如 "openai"、"azure"、"anthropic"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// 模型的 API 类型。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "apiType")]
    pub api_type: Option<ApiType>,
    /// 用于认证的连接配置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<Connection>,
    /// 生成选项（temperature、max_tokens 等）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<ModelOptions>,
}

/// 模型端点的 API 类型鉴别器。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiType {
    /// 标准聊天补全 API。
    Chat,
    /// Azure OpenAI Responses API。
    Responses,
}

/// 控制生成行为的选项，与 MAF AgentSchema v1.0 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelOptions {
    /// ModelOptions 的类型鉴别器。
    pub kind: String,
    /// Token 频率惩罚（-2.0 到 2.0）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    /// 输出的最大 token 数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    /// Token 存在惩罚（-2.0 到 2.0）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    /// 用于确定性输出的随机种子。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i32>,
    /// 采样温度（0.0 到 2.0）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Top-K 采样值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    /// Top-P（核）采样值（0.0 到 1.0）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// 停止序列，用于中止生成。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// 是否允许每次响应多次工具调用。
    #[serde(default)]
    pub allow_multiple_tool_calls: Option<bool>,
    /// 用于向前兼容的额外自定义属性。
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Model {
    /// 仅用 ID 创建模型。
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider: None,
            api_type: None,
            connection: None,
            options: None,
        }
    }

    /// 设置提供商。
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// 设置连接。
    pub fn with_connection(mut self, connection: Connection) -> Self {
        self.connection = Some(connection);
        self
    }

    /// 设置模型选项。
    pub fn with_options(mut self, options: ModelOptions) -> Self {
        self.options = Some(options);
        self
    }
}

impl ModelOptions {
    /// 用类型标签创建模型选项。
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

    /// 设置 temperature。
    pub fn with_temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }

    /// 设置最大输出 token 数。
    pub fn with_max_output_tokens(mut self, tokens: i32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    /// 设置 top-p。
    pub fn with_top_p(mut self, top_p: f64) -> Self {
        self.top_p = Some(top_p);
        self
    }
}

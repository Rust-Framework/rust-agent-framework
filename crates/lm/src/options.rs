use rust_agent_core::ModelMetadata;
use serde::{Deserialize, Serialize};

/// 本地 lm.rs 推理客户端配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LmChatClientOptions {
    /// LMRS 格式模型权重文件路径。
    pub model_path: String,
    /// LMRS 格式 tokenizer 文件路径。
    pub tokenizer_path: String,
    /// 逻辑模型标识（用于日志与元数据）。
    pub model_id: String,
    /// 默认采样温度（0 = greedy）。
    pub temperature: Option<f32>,
    /// 默认 nucleus top-p。
    pub top_p: Option<f32>,
    /// 默认最大生成 token 数。
    pub max_tokens: Option<u32>,
    /// 随机种子；`None` 表示每次运行使用随机种子。
    pub seed: Option<u64>,
    /// 模型能力元数据（上下文窗口等）。
    #[serde(skip)]
    pub model_metadata: Option<ModelMetadata>,
}

impl LmChatClientOptions {
    pub fn new(
        model_path: impl Into<String>,
        tokenizer_path: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Self {
        Self {
            model_path: model_path.into(),
            tokenizer_path: tokenizer_path.into(),
            model_id: model_id.into(),
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(512),
            seed: None,
            model_metadata: None,
        }
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_top_p(mut self, top_p: f32) -> Self {
        self.top_p = Some(top_p);
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn with_model_metadata(mut self, metadata: ModelMetadata) -> Self {
        self.model_metadata = Some(metadata);
        self
    }
}

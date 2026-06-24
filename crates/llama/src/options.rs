use rust_agent_core::ModelMetadata;
use serde::{Deserialize, Serialize};

/// 本地 GGUF 推理客户端配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlamaChatClientOptions {
    /// GGUF 模型文件路径（`.gguf`）。
    pub model_path: String,
    /// 可选外部 tokenizer 路径；GGUF 内嵌 tokenizer 时留空。
    pub tokenizer_path: Option<String>,
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
    /// 是否尝试使用 GPU 后端。
    pub use_gpu: Option<bool>,
    /// 最大上下文长度覆盖。
    pub max_context_len: Option<usize>,
    /// 模型能力元数据（上下文窗口等）。
    #[serde(skip)]
    pub model_metadata: Option<ModelMetadata>,
}

impl LlamaChatClientOptions {
    pub fn new(model_path: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            model_path: model_path.into(),
            tokenizer_path: None,
            model_id: model_id.into(),
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(512),
            seed: None,
            use_gpu: Some(false),
            max_context_len: None,
            model_metadata: None,
        }
    }

    pub fn with_tokenizer_path(mut self, path: impl Into<String>) -> Self {
        self.tokenizer_path = Some(path.into());
        self
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

    pub fn with_use_gpu(mut self, use_gpu: bool) -> Self {
        self.use_gpu = Some(use_gpu);
        self
    }

    pub fn with_max_context_len(mut self, len: usize) -> Self {
        self.max_context_len = Some(len);
        self
    }

    pub fn with_model_metadata(mut self, metadata: ModelMetadata) -> Self {
        self.model_metadata = Some(metadata);
        self
    }
}

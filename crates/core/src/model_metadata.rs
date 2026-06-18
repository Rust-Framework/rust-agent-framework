/// 描述 LLM 模型能力边界的元数据。
///
/// 压缩策略使用它计算令牌预算，框架使用它强制上下文窗口限制。
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    /// 模型标识符（如 "gpt-4o"、"deepseek-chat"）
    pub model_id: String,
    /// 最大上下文窗口大小（令牌数）
    pub context_window_tokens: usize,
    /// 模型可生成的最大输出令牌数
    pub max_output_tokens: usize,
}

impl ModelMetadata {
    pub fn new(model_id: impl Into<String>, context_window_tokens: usize, max_output_tokens: usize) -> Self {
        Self {
            model_id: model_id.into(),
            context_window_tokens,
            max_output_tokens,
        }
    }

    /// 输入令牌预算 = 上下文窗口 - 最大输出。
    ///
    /// 这是输入消息（系统提示 + 历史记录 + 用户消息 + 注入上下文）
    /// 可用的最大令牌数。
    pub fn input_budget(&self) -> usize {
        self.context_window_tokens.saturating_sub(self.max_output_tokens)
    }
}

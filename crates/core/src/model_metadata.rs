/// Model metadata describing an LLM model's capability boundaries.
///
/// Used by compression strategies to calculate token budgets and by
/// the framework to enforce context window limits.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    /// The model identifier (e.g. "gpt-4o", "deepseek-chat")
    pub model_id: String,
    /// Maximum context window size in tokens
    pub context_window_tokens: usize,
    /// Maximum output tokens the model can generate
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

    /// Input token budget = context window - max output.
    ///
    /// This is the maximum number of tokens available for input messages
    /// (system prompt + history + user message + injected context).
    pub fn input_budget(&self) -> usize {
        self.context_window_tokens.saturating_sub(self.max_output_tokens)
    }
}

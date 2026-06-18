use rust_agent_core::{ChatMessage, ICompressionStrategy, ITokenCounter, Result};

/// 链式组合多个策略的压缩管道。
///
/// 策略按顺序应用。每个策略接收前一个策略的输出。
/// 这允许将简单策略组合成更复杂的压缩行为。
///
/// 示例：SlidingWindow（粗粒度）→ TokenBudget（细粒度）
pub struct CompressionPipeline {
    strategies: Vec<Box<dyn ICompressionStrategy>>,
}

impl CompressionPipeline {
    pub fn new() -> Self {
        Self {
            strategies: Vec::new(),
        }
    }

    /// Add a strategy to the pipeline.
    pub fn add_strategy(mut self, strategy: Box<dyn ICompressionStrategy>) -> Self {
        self.strategies.push(strategy);
        self
    }

    /// Build an empty pipeline (no compression).
    pub fn noop() -> Self {
        Self::new()
    }
}

impl Default for CompressionPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl ICompressionStrategy for CompressionPipeline {
    fn name(&self) -> &str {
        "CompressionPipeline"
    }

    fn compress(
        &self,
        mut messages: Vec<ChatMessage>,
        budget: usize,
        counter: &dyn ITokenCounter,
    ) -> Result<Vec<ChatMessage>> {
        for strategy in &self.strategies {
            let current_tokens = counter.count_tokens(&messages);
            if current_tokens <= budget {
                tracing::debug!(
                    strategy = strategy.name(),
                    "Skipping compression — already within budget"
                );
                break;
            }

            tracing::debug!(
                strategy = strategy.name(),
                before_tokens = current_tokens,
                budget = budget,
                "Applying compression strategy"
            );

            messages = strategy.compress(messages, budget, counter)?;
        }
        Ok(messages)
    }
}

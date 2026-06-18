use crate::{ChatMessage, ITokenCounter, Result};

/// 上下文压缩策略接口。
///
/// 将消息列表压缩至令牌预算内，保留最重要的上下文，
/// 同时丢弃或摘要较旧的消息。
///
/// 实现通过 `CompressionPipeline` 链式组合，
/// 并集成到 `ChatClientAgent` 的 Phase 1.5 阶段。
pub trait ICompressionStrategy: Send + Sync {
    /// 人类可读的名称，用于日志记录和诊断。
    fn name(&self) -> &str;

    /// 将消息压缩至给定的令牌预算内。
    ///
    /// `budget` 是压缩后消息列表应占用的最大令牌数。
    /// 策略应尽力保持在预算内，但不保证一定满足
    /// （例如，单条消息超过预算时）。
    ///
    /// 返回压缩后的消息列表。
    fn compress(
        &self,
        messages: Vec<ChatMessage>,
        budget: usize,
        counter: &dyn ITokenCounter,
    ) -> Result<Vec<ChatMessage>>;
}

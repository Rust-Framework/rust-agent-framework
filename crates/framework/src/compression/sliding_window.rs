use rust_agent_core::{ChatMessage, ICompressionStrategy, ITokenCounter, MessageRole, Result};

/// 滑动窗口压缩策略。
///
/// 仅保留最近的 N 条消息，丢弃更早的消息。
/// 系统消息始终保留，不受窗口大小影响。
///
/// 这是最简单的压缩策略——适用于仅需最近上下文的场景。
pub struct SlidingWindowStrategy {
    /// 保留的非系统消息最大数量。
    pub max_messages: usize,
}

impl SlidingWindowStrategy {
    pub fn new(max_messages: usize) -> Self {
        Self { max_messages }
    }
}

impl ICompressionStrategy for SlidingWindowStrategy {
    fn name(&self) -> &str {
        "SlidingWindow"
    }

    fn compress(
        &self,
        messages: Vec<ChatMessage>,
        _budget: usize,
        _counter: &dyn ITokenCounter,
    ) -> Result<Vec<ChatMessage>> {
        // Separate system messages from the rest
        let mut system_msgs: Vec<ChatMessage> = Vec::new();
        let mut other_msgs: Vec<ChatMessage> = Vec::new();

        for msg in messages {
            if msg.role == MessageRole::System {
                system_msgs.push(msg);
            } else {
                other_msgs.push(msg);
            }
        }

        // If within limit, return as-is
        if other_msgs.len() <= self.max_messages {
            system_msgs.extend(other_msgs);
            return Ok(system_msgs);
        }

        // Keep only the most recent messages
        let keep_from = other_msgs.len().saturating_sub(self.max_messages);
        let recent: Vec<ChatMessage> = other_msgs.into_iter().skip(keep_from).collect();

        tracing::info!(
            strategy = self.name(),
            kept = recent.len(),
            discarded = keep_from,
            "Sliding window compression applied"
        );

        system_msgs.extend(recent);
        Ok(system_msgs)
    }
}

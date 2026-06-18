use crate::ChatMessage;

/// 令牌计数器接口，用于在发送给 LLM 前预估令牌消耗。
///
/// 压缩策略使用它做出关于保留或截断消息的决策。
pub trait ITokenCounter: Send + Sync {
    /// 计算消息列表的总令牌数。
    ///
    /// 包括消息格式开销（角色标签、分隔符等）以及内容本身。
    fn count_tokens(&self, messages: &[ChatMessage]) -> usize;

    /// 计算纯文本字符串的令牌数。
    ///
    /// 用于在不需要构造完整 `ChatMessage` 对象的情况下，
    /// 估算指令或注入上下文的令牌成本。
    fn count_text_tokens(&self, text: &str) -> usize;
}

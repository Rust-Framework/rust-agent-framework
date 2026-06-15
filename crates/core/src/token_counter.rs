use crate::ChatMessage;

/// Token counter interface for estimating token consumption before sending to LLM.
///
/// Used by compression strategies to make informed decisions about
/// how many messages to retain or truncate.
pub trait ITokenCounter: Send + Sync {
    /// Count the total tokens for a list of messages.
    ///
    /// Includes message formatting overhead (role labels, separators, etc.)
    /// as well as the content itself.
    fn count_tokens(&self, messages: &[ChatMessage]) -> usize;

    /// Count tokens for a plain text string.
    ///
    /// Useful for estimating token cost of instructions or injected context
    /// without constructing full `ChatMessage` objects.
    fn count_text_tokens(&self, text: &str) -> usize;
}

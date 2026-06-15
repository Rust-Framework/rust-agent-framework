use crate::{ChatMessage, ITokenCounter, Result};

/// Context compression strategy interface.
///
/// Compresses a list of messages to fit within a token budget,
/// preserving the most important context while discarding or
/// summarizing older messages.
///
/// Implementations are chained via `CompressionPipeline` and
/// integrated into `ChatClientAgent` Phase 1.5.
pub trait ICompressionStrategy: Send + Sync {
    /// Human-readable name for logging and diagnostics.
    fn name(&self) -> &str;

    /// Compress messages to fit within the given token budget.
    ///
    /// `budget` is the maximum number of tokens the compressed
    /// message list should occupy. The strategy should make a
    /// best-effort attempt to stay within this budget, but is
    /// not required to guarantee it (e.g., if a single message
    /// exceeds the budget).
    ///
    /// Returns the compressed message list.
    fn compress(
        &self,
        messages: Vec<ChatMessage>,
        budget: usize,
        counter: &dyn ITokenCounter,
    ) -> Result<Vec<ChatMessage>>;
}

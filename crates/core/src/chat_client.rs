use async_trait::async_trait;
use crate::{BoxStream, ChatMessage, ChatStreamChunk, Result};

/// Chat client interface following MAF's ChatClient abstraction.
///
/// A thin wrapper over LLM provider APIs.
/// Only streaming output is supported.
#[async_trait]
pub trait IChatClient: Send + Sync {
    /// Run chat completion and produce a stream of chunks.
    async fn run(&self, messages: &[ChatMessage]) -> Result<BoxStream<Result<ChatStreamChunk>>>;

    /// The model identifier used by this client.
    fn model_id(&self) -> &str;
}

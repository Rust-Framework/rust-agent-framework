use async_trait::async_trait;
use crate::{AgentId, AgentMetadata, AgentStreamChunk, BoxStream, ChatMessage, Result};

/// Core agent interface following MAF design.
///
/// An agent is an autonomous software component that can reason,
/// plan, and execute using LLMs, tools, and other agents.
/// Only streaming output is supported.
#[async_trait]
pub trait IAgent: Send + Sync {
    fn id(&self) -> &AgentId;
    fn metadata(&self) -> &AgentMetadata;

    /// Process messages and produce a streaming response.
    async fn run(&self, messages: Vec<ChatMessage>) -> Result<BoxStream<Result<AgentStreamChunk>>>;

    /// Reset the agent's internal state.
    async fn reset(&self) -> Result<()>;
}

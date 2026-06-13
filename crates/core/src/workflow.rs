use async_trait::async_trait;
use crate::{AgentId, AgentStreamChunk, BoxStream, ChatAgentRunOptions, ChatMessage, Result};

/// Workflow interface following MAF's graph-based orchestration.
///
/// Workflows connect multiple agents and functions via
/// typed data-flow edges. Only streaming output is supported.
#[async_trait]
pub trait IWorkflow: Send + Sync {
    /// Execute the workflow and produce a streaming response.
    async fn run(
        &self,
        input: Vec<ChatMessage>,
        options: ChatAgentRunOptions,
    ) -> Result<BoxStream<Result<AgentStreamChunk>>>;

    /// Execute the workflow starting from a specific agent.
    async fn run_from(
        &self,
        agent_id: &AgentId,
        input: Vec<ChatMessage>,
        options: ChatAgentRunOptions,
    ) -> Result<BoxStream<Result<AgentStreamChunk>>>;
}

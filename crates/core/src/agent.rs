use async_trait::async_trait;
use std::sync::Arc;

use crate::session::ISession;
use crate::{AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, Result};

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
    ///
    /// `session` is an optional session for maintaining conversation history.
    /// `options` allows per-call overrides (instructions, temperature, etc.)
    /// without mutating the agent's persistent state. Pass `None` for
    /// standard behaviour.
    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>>;

    /// Get a sub-agent by id.
    fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>>;

    /// List all sub-agents.
    fn list_subagents(&self) -> Vec<Arc<dyn IAgent>>;

    /// Reset the agent's internal state.
    async fn reset(&self) -> Result<()>;
}

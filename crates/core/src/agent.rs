use async_trait::async_trait;
use std::sync::Arc;

use crate::session::{AgentSession, ISession};
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
    ///
    /// Sub-agents enable multi-agent orchestration — a parent agent can
    /// delegate to child agents, and frontends can discover the agent tree
    /// to provide interactive sub-agent inspection, streaming output, and
    /// execution status tracking.
    ///
    /// Default returns `None`. Override in agents that manage child agents
    /// (e.g., `GraphFlow`, `WorkflowAgent`).
    fn get_subagent(&self, _id: &AgentId) -> Option<Arc<dyn IAgent>> {
        None
    }

    /// Reset the agent's internal state.
    async fn reset(&self) -> Result<()>;

    /// Create a new session for this agent.
    ///
    /// Default implementation creates an `AgentSession` with a random UUID.
    /// Override to create agent-specific session types.
    fn create_session(&self) -> Arc<dyn ISession> {
        Arc::new(AgentSession::new())
    }

    /// Deserialize a session from serialized data.
    ///
    /// Default implementation deserializes as `AgentSession`.
    /// Override to support agent-specific session types.
    fn deserialize_session(&self, data: &str) -> Result<Arc<dyn ISession>> {
        let session = AgentSession::deserialize(data)?;
        Ok(Arc::new(session))
    }
}

use async_trait::async_trait;
use std::sync::Arc;

use rust_agent_core::{AgentId, AgentStreamChunk, BoxStream, ChatAgentRunOptions, ChatMessage, IAgent, IWorkflow, Result};
use rust_agent_framework::AgentRuntime;

/// GraphFlow — graph-based workflow engine following MAF's Workflow layer.
///
/// **Note:** Current implementation is an MVP. It supports agent registration
/// and entry-point execution, but does not yet support edges, conditions,
/// or multi-step graph traversal.
///
/// Internally delegates agent management to `AgentRuntime`.
pub struct GraphFlow {
    runtime: AgentRuntime,
    entry_agent: Option<AgentId>,
}

impl GraphFlow {
    pub fn new() -> Self {
        Self { runtime: AgentRuntime::new(), entry_agent: None }
    }

    pub fn add_agent(&mut self, agent: Arc<dyn IAgent>) {
        self.runtime.register_agent(agent);
    }

    pub fn set_entry(&mut self, agent_id: AgentId) {
        self.entry_agent = Some(agent_id);
    }

    pub fn get_agent(&self, agent_id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.runtime.get_agent(agent_id)
    }
}

impl Default for GraphFlow {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl IWorkflow for GraphFlow {
    async fn run(
        &self,
        input: Vec<ChatMessage>,
        options: ChatAgentRunOptions,
    ) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        let entry_id = self.entry_agent.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::WorkflowError("No entry agent configured".to_string())
        })?;
        self.run_from(entry_id, input, options).await
    }

    async fn run_from(
        &self,
        agent_id: &AgentId,
        input: Vec<ChatMessage>,
        options: ChatAgentRunOptions,
    ) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        self.runtime.run(agent_id, input, options).await
    }
}

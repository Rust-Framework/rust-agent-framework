use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{AgentId, AgentStreamChunk, BoxStream, ChatMessage, IAgent, IWorkflow, Result};

/// GraphFlow — graph-based workflow engine following MAF's Workflow layer.
pub struct GraphFlow {
    agents: HashMap<AgentId, Arc<dyn IAgent>>,
    entry_agent: Option<AgentId>,
}

impl GraphFlow {
    pub fn new() -> Self { Self { agents: HashMap::new(), entry_agent: None } }

    pub fn add_agent(&mut self, agent: Arc<dyn IAgent>) {
        self.agents.insert(agent.id().clone(), agent);
    }

    pub fn set_entry(&mut self, agent_id: AgentId) {
        self.entry_agent = Some(agent_id);
    }

    pub fn get_agent(&self, agent_id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.agents.get(agent_id)
    }
}

impl Default for GraphFlow {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl IWorkflow for GraphFlow {
    async fn run(&self, input: Vec<ChatMessage>) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        let entry_id = self.entry_agent.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::WorkflowError("No entry agent configured".to_string())
        })?;
        self.run_from(entry_id, input).await
    }

    async fn run_from(
        &self,
        agent_id: &AgentId,
        input: Vec<ChatMessage>,
    ) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        let agent = self.agents.get(agent_id).ok_or_else(|| {
            rust_agent_core::AgentError::AgentNotFound(agent_id.to_string())
        })?;
        agent.run(input).await
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{AgentId, AgentStreamChunk, BoxStream, ChatMessage, IAgent, Result};

/// AgentRuntime — the execution host for agents following MAF.
///
/// Manages agent registration, message routing, and lifecycle.
pub struct AgentRuntime {
    agents: HashMap<AgentId, Arc<dyn IAgent>>,
}

impl AgentRuntime {
    pub fn new() -> Self { Self { agents: HashMap::new() } }

    pub fn register_agent(&mut self, agent: Arc<dyn IAgent>) {
        self.agents.insert(agent.id().clone(), agent);
    }

    pub fn get_agent(&self, id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.agents.get(id)
    }

    /// Run a message against a specific agent.
    pub async fn run(
        &self,
        agent_id: &AgentId,
        messages: Vec<ChatMessage>,
    ) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        let agent = self.agents.get(agent_id).ok_or_else(|| {
            rust_agent_core::AgentError::AgentNotFound(agent_id.to_string())
        })?;
        agent.run(messages).await
    }

    pub fn agent_ids(&self) -> Vec<&AgentId> {
        self.agents.keys().collect()
    }
}

impl Default for AgentRuntime {
    fn default() -> Self { Self::new() }
}

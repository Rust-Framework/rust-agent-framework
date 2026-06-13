use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentStreamChunk, BoxStream, ChatMessage, IAgent, Result,
};

/// Handoff orchestration pattern — one agent decides which agent runs next.
/// Corresponds to MAF's handoff pattern from OpenAI Swarm.
///
/// **Note:** Current implementation is a placeholder. The triage agent's
/// response is not yet parsed to determine routing. Only the triage agent
/// is invoked directly.
pub struct HandoffPattern {
    agents: Vec<Arc<dyn IAgent>>,
    triage_index: usize,
}

impl HandoffPattern {
    pub fn new(agents: Vec<Arc<dyn IAgent>>, triage_index: usize) -> Self {
        Self { agents, triage_index }
    }

    /// Execute the handoff pattern: triage agent routes to the target agent.
    pub async fn run(&self, input: Vec<ChatMessage>) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        let triage = self.agents.get(self.triage_index).ok_or_else(|| {
            rust_agent_core::AgentError::WorkflowError("Invalid triage agent index".to_string())
        })?;

        // Triage agent decides which agent to hand off to
        triage.run(input).await
    }

    pub fn find_agent(&self, id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.agents.iter().find(|a| a.id() == id)
    }
}

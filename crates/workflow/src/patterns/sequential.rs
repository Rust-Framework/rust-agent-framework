use std::sync::Arc;

use rust_agent_core::{
    collect_agent_response, AgentStreamChunk, BoxStream, ChatAgentRunOptions, ChatMessage, IAgent, Result,
};

/// Sequential orchestration pattern — agents run in order,
/// each receiving the collected output of the previous agent.
pub struct SequentialPattern {
    agents: Vec<Arc<dyn IAgent>>,
}

impl SequentialPattern {
    pub fn new(agents: Vec<Arc<dyn IAgent>>) -> Self {
        Self { agents }
    }

    /// Execute agents sequentially, piping collected output forward.
    pub async fn run(&self, input: Vec<ChatMessage>, options: ChatAgentRunOptions) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        let mut messages = input;

        // Run all but the last agent, collecting their output
        for (i, agent) in self.agents.iter().enumerate() {
            let is_last = i == self.agents.len() - 1;
            let stream = agent.run(messages, options.clone()).await?;

            if is_last {
                return Ok(stream);
            }

            // Collect the intermediate response for piping
            let response = collect_agent_response(stream).await?;
            messages = vec![ChatMessage::assistant(&response.text)];
        }

        Err(rust_agent_core::AgentError::WorkflowError(
            "No agents in sequential pattern".to_string(),
        ))
    }
}

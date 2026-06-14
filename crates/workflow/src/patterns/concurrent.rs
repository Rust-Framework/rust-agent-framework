use std::sync::Arc;

use rust_agent_core::{AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent, Result};

/// Concurrent (fan-out/fan-in) orchestration pattern —
/// agents run in parallel, streams are merged.
pub struct ConcurrentPattern {
    agents: Vec<Arc<dyn IAgent>>,
}

impl ConcurrentPattern {
    pub fn new(agents: Vec<Arc<dyn IAgent>>) -> Self {
        Self { agents }
    }

    /// Execute agents concurrently and merge their streams.
    pub async fn run(
        &self,
        input: Vec<ChatMessage>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let mut streams = Vec::new();

        for agent in &self.agents {
            let s = agent.run(input.clone(), None, options.clone()).await?;
            streams.push(s);
        }

        // Merge all streams into one
        let merged = futures_util::stream::select_all(streams);
        Ok(Box::pin(merged))
    }
}

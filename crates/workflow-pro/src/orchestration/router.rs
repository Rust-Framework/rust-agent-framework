use std::sync::Arc;
use async_trait::async_trait;
use rust_agent_core::{IAgent, Result};
use rust_agent_workflow::engine::message_envelope::MessageEnvelope;

pub struct DynamicRouter {
    agents: Vec<Arc<dyn IAgent>>,
    capability_keyword: String,
}

impl DynamicRouter {
    pub fn new(agents: Vec<Arc<dyn IAgent>>, capability_keyword: impl Into<String>) -> Self {
        Self { agents, capability_keyword: capability_keyword.into() }
    }

    pub fn route(&self, input_text: &str) -> Option<&Arc<dyn IAgent>> {
        let lower = input_text.to_lowercase();
        for agent in &self.agents {
            let meta = agent.metadata();
            if meta.description.to_lowercase().contains(&lower)
                || meta.agent_type.to_lowercase().contains(&lower)
                || meta.capability_tags.iter().any(|t| t.to_lowercase() == lower)
            {
                return Some(agent);
            }
        }
        self.agents.first()
    }
}

#[async_trait]
impl rust_agent_workflow::graph::edge::IEdgeCondition for DynamicRouter {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> Result<bool> {
        if let Some(text) = envelope.content.downcast_ref::<String>() {
            return Ok(text.to_lowercase().contains(&self.capability_keyword.to_lowercase()));
        }
        Ok(false)
    }
}
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent,
    ISession, Result,
};

/// GraphFlow — graph-based workflow engine following MAF's Workflow layer.
///
/// **Note:** Current implementation is an MVP. It supports agent registration
/// and entry-point execution, but does not yet support edges, conditions,
/// or multi-step graph traversal.
///
/// Implements IAgent so it can be used as a sub-agent in larger workflows.
pub struct GraphFlow {
    id: AgentId,
    metadata: AgentMetadata,
    agents: HashMap<AgentId, Arc<dyn IAgent>>,
    entry_agent: Option<AgentId>,
}

impl GraphFlow {
    pub fn new() -> Self {
        Self {
            id: AgentId::new("graph_flow"),
            metadata: AgentMetadata {
                agent_type: "GraphFlow".to_string(),
                key: "graph_flow".to_string(),
                description: String::new(),
            },
            agents: HashMap::new(),
            entry_agent: None,
        }
    }

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
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IAgent for GraphFlow {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let entry_id = self.entry_agent.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::WorkflowError("No entry agent configured".to_string())
        })?;
        self.run_from(entry_id, messages, session, options).await
    }

    fn get_subagent(&self, agent_id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.agents.get(agent_id).cloned()
    }

    fn list_subagents(&self) -> Vec<Arc<dyn IAgent>> {
        self.agents.values().cloned().collect()
    }

    async fn reset(&self) -> Result<()> {
        Ok(())
    }
}

impl GraphFlow {
    /// Execute the workflow starting from a specific agent.
    pub async fn run_from(
        &self,
        agent_id: &AgentId,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let agent = self.agents.get(agent_id).ok_or_else(|| {
            rust_agent_core::AgentError::AgentNotFound(agent_id.to_string())
        })?;
        agent.run(messages, session, options).await
    }
}

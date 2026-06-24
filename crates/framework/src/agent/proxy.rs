use async_trait::async_trait;
use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage,
    IAgent, IChatClient, ISession, Result,
};

/// Lightweight agent handle passed to `on_invoked` callbacks.
pub(crate) struct AgentProxy {
    pub(crate) id: AgentId,
    pub(crate) metadata: AgentMetadata,
    pub(crate) chat_client: Arc<dyn IChatClient>,
}

#[async_trait]
impl IAgent for AgentProxy {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    fn chat_client(&self) -> Option<&Arc<dyn IChatClient>> {
        Some(&self.chat_client)
    }

    async fn run(
        &self,
        _: Vec<ChatMessage>,
        _: Option<Arc<dyn ISession>>,
        _: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        Err(rust_agent_core::AgentError::ConfigError(
            "AgentProxy::run not supported".into(),
        ))
    }

    async fn reset(&self) -> Result<()> {
        Ok(())
    }
}

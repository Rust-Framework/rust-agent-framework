use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, Result,
};

/// Forces deterministic LLM parameters for bundle curation file writes.
///
/// - temperature → 0.1
/// - thinking disabled
/// - parallel_tool_calls → false
pub(crate) struct CuratorChatClient {
    inner: Arc<dyn IChatClient>,
}

impl CuratorChatClient {
    pub fn new(inner: Arc<dyn IChatClient>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl IChatClient for CuratorChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        mut options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        options.temperature = Some(0.1);
        options.parallel_tool_calls = Some(false);
        options.extra_body.remove("thinking");
        options.extra_body.remove("reasoning_effort");

        self.inner.run(messages, options).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn inner_client(&self) -> Option<&Arc<dyn IChatClient>> {
        Some(&self.inner)
    }
}

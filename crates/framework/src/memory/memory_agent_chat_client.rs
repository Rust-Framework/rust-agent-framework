use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, Result,
};

/// Light-weight decorator that forces precision parameters on every call.
///
/// Memory consolidation demands near-deterministic output so the same facts
/// consistently produce the same file writes (no hallucinated records):
///
/// - **temperature → 0.3**:  near-deterministic with slight flexibility for structured writes.
/// - **thinking → disabled**:  memory consolidation is file IO, not open-ended reasoning.
/// - **parallel_tool_calls → false**:  one tool at a time; avoids empty `{}` bursts.
///
/// ## Relationship to `with_memory_agent()`
///
/// When the user does NOT call `with_memory_agent()`, `SkillMemoryContextProvider`
/// auto-discovers the main agent's `IChatClient` and wraps it in this decorator.
/// When `with_memory_agent()` IS called, the provider respects the user's
/// custom client as-is (advanced override) — the user is responsible for
/// configuring its parameters.
pub(crate) struct MemoryAgentChatClient {
    inner: Arc<dyn IChatClient>,
}

impl MemoryAgentChatClient {
    pub fn new(inner: Arc<dyn IChatClient>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl IChatClient for MemoryAgentChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        mut options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        options.temperature = Some(0.3);
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

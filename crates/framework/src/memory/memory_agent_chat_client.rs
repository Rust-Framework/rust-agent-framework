use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient, Result,
};

/// Light-weight decorator that forces memory-optimal parameters on every call.
///
/// Memory consolidation is a precision task — the agent needs to distill
/// conversation facts into concise, accurate records.  High temperature and
/// chain-of-thought reasoning are actively harmful here:
///
/// - **temperature → 0.1**:  near-deterministic output so the same facts
///   consistently produce the same file writes (no hallucinated records).
/// - **thinking → disabled**:  extra reasoning tokens are noise, not signal;
///   the `AGENT.md` prompt already encodes the consolidation workflow.
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
        // Force memory-optimal parameters
        options.temperature = Some(0.1);

        // Disable chain-of-thought / reasoning — wastes tokens on a
        // structured write task whose workflow is already encoded in
        // the AGENT.md system prompt.
        options
            .extra_body
            .insert("thinking".to_string(), serde_json::json!({ "type": "disabled" }));
        options.extra_body.remove("reasoning_effort");

        self.inner.run(messages, options).await
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }
}

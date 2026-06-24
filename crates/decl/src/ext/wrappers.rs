use std::sync::Arc;

use rust_agent_core::{IChatClient, ITool};

/// Wraps `Arc<dyn IChatClient>` to implement `IChatClient`, for use with `AgentBuilder<C>`.
pub struct ChatClientWrapper(pub Arc<dyn IChatClient>);

#[async_trait::async_trait]
impl IChatClient for ChatClientWrapper {
    fn model_id(&self) -> &str {
        self.0.model_id()
    }

    fn model_metadata(&self) -> Option<&rust_agent_core::ModelMetadata> {
        self.0.model_metadata()
    }

    async fn run(
        &self,
        messages: &[rust_agent_core::ChatMessage],
        options: rust_agent_core::ChatClientRunOptions,
    ) -> rust_agent_core::Result<
        rust_agent_core::BoxStream<
            'static,
            rust_agent_core::Result<rust_agent_core::AgentResponseUpdate>,
        >,
    > {
        self.0.run(messages, options).await
    }

    fn inner_client(&self) -> Option<&Arc<dyn IChatClient>> {
        Some(&self.0)
    }
}

/// Wraps `Arc<dyn ITool>` to implement `ITool`, for use with `AgentBuilder::with_tool()`.
pub struct ToolWrapper(pub Arc<dyn ITool>);

#[async_trait::async_trait]
impl ITool for ToolWrapper {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn parameters(&self) -> serde_json::Value {
        self.0.parameters()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<rust_agent_core::ToolResult> {
        self.0.execute(arguments).await
    }

    fn kind(&self) -> &str {
        self.0.kind()
    }
}

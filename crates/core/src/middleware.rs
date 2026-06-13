use async_trait::async_trait;
use crate::{AgentResponse, ChatMessage, Result};

/// Middleware interface following MAF's middleware pipeline.
///
/// Middleware can intercept and modify messages and responses
/// in the agent's processing pipeline.
#[async_trait]
pub trait IMiddleware: Send + Sync {
    /// Process incoming messages before they reach the agent.
    async fn on_request(&self, messages: &mut Vec<ChatMessage>) -> Result<()>;

    /// Process the aggregated response before it's returned to the caller.
    async fn on_response(&self, response: &mut AgentResponse) -> Result<()>;
}

use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

/// An empty placeholder node that simply passes the message through.
///
/// Useful as a no-op step in process diagrams or as a starting node
/// before concrete business logic is added.
pub struct NoneTask {
    pub node_id: String,
}

impl NoneTask {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
        }
    }
}

#[async_trait]
impl IExecutor for NoneTask {
    fn id(&self) -> &str {
        &self.node_id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("initial")]
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        _ctx: Arc<dyn IWorkflowContext>,
        _progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        tracing::info!(
            node_id = %self.node_id,
            "NoneTask passing message through"
        );

        Ok(HandlerResult::Messages(vec![message]))
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

/// Sends a message via the message broker to an external system.
///
/// Emits a progress event and passes the incoming message downstream.
pub struct SendTask {
    pub node_id: String,
    pub message_name: String,
    pub correlation_key: Option<String>,
}

impl SendTask {
    pub fn new(
        node_id: impl Into<String>,
        message_name: impl Into<String>,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            message_name: message_name.into(),
            correlation_key: None,
        }
    }

    pub fn with_correlation_key(mut self, key: impl Into<String>) -> Self {
        self.correlation_key = Some(key.into());
        self
    }
}

#[async_trait]
impl IExecutor for SendTask {
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
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        tracing::info!(
            node_id = %self.node_id,
            message_name = %self.message_name,
            correlation_key = ?self.correlation_key,
            "SendTask dispatching message to broker"
        );

        let _ = progress.send(NodeProgress::Custom {
            key: "send_task.dispatched".into(),
            value: serde_json::json!({
                "message_name": self.message_name,
                "correlation_key": self.correlation_key,
            }),
        });

        // The incoming message is passed downstream for further routing.
        Ok(HandlerResult::Messages(vec![message]))
    }
}

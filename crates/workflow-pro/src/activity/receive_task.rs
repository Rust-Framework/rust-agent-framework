use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;
use rust_agent_workflow::engine::correlation::CorrelationKey;

/// Waits for a correlated message from an external system.
///
/// Uses the workflow engine's `MessageCorrelation` and boundary event
/// semantics. On first call: requests halt. On resume with a matching
/// correlated message: passes it through to the next node.
pub struct ReceiveTask {
    pub node_id: String,
    pub correlation_key: CorrelationKey,
    pub timeout: Option<Duration>,
    awaiting: Mutex<bool>,
}

impl ReceiveTask {
    pub fn new(node_id: impl Into<String>, correlation_key: CorrelationKey) -> Self {
        Self {
            node_id: node_id.into(),
            correlation_key,
            timeout: None,
            awaiting: Mutex::new(false),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

#[async_trait]
impl IExecutor for ReceiveTask {
    fn id(&self) -> &str {
        &self.node_id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![
            TypeTag::new("initial"),
            TypeTag::new("resume"),
        ]
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        _progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let is_resume = *self.awaiting.lock();

        if is_resume {
            // Resume: a correlated message has arrived — pass it through
            *self.awaiting.lock() = false;

            tracing::info!(
                node_id = %self.node_id,
                "ReceiveTask resumed with correlated message"
            );

            return Ok(HandlerResult::Messages(vec![message]));
        }

        // First call: register correlation and halt, waiting for message
        tracing::info!(
            node_id = %self.node_id,
            correlation_key = ?self.correlation_key,
            timeout = ?self.timeout,
            "ReceiveTask halting and waiting for correlated message"
        );

        // Schedule a timeout timer if configured
        if let Some(duration) = self.timeout {
            ctx.schedule_timer("receive_timeout", duration).await?;
        }

        ctx.request_halt().await;
        *self.awaiting.lock() = true;

        Ok(HandlerResult::None)
    }
}

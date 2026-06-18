use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

/// Calls a sub-process (child workflow) by its definition ID.
///
/// On handle(), loads the sub-process definition from the `IProcessRepository`
/// and delegates execution. This is a placeholder that logs the intent
/// and passes the message through.
pub struct CallActivity {
    pub node_id: String,
    pub sub_process_id: String,
}

impl CallActivity {
    pub fn new(
        node_id: impl Into<String>,
        sub_process_id: impl Into<String>,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            sub_process_id: sub_process_id.into(),
        }
    }
}

#[async_trait]
impl IExecutor for CallActivity {
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
            sub_process_id = %self.sub_process_id,
            "CallActivity delegating to sub-process"
        );

        let _ = progress.send(NodeProgress::Custom {
            key: "call_activity.invoking".into(),
            value: serde_json::json!({
                "sub_process_id": self.sub_process_id,
            }),
        });

        // Placeholder: in production, this would:
        // 1. Look up the sub-process definition via IProcessRepository
        // 2. Create a WorkflowEngine from the sub-process graph
        // 3. Execute it with the incoming message as input
        // 4. Collect outputs and pass them downstream
        tracing::info!(
            node_id = %self.node_id,
            sub_process_id = %self.sub_process_id,
            "CallActivity placeholder — sub-process delegation not yet wired"
        );

        Ok(HandlerResult::Messages(vec![message]))
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

/// Executes an inline script (default: rhai).
///
/// handle() logs the script execution details and passes the message through.
/// Actual rhai integration is a placeholder.
pub struct ScriptTask {
    pub node_id: String,
    pub script: String,
    pub language: String,
}

impl ScriptTask {
    pub fn new(node_id: impl Into<String>, script: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            script: script.into(),
            language: "rhai".to_string(),
        }
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }
}

#[async_trait]
impl IExecutor for ScriptTask {
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
            language = %self.language,
            script_len = %self.script.len(),
            "ScriptTask executing inline script"
        );

        let _ = progress.send(NodeProgress::Custom {
            key: "script_task.executing".into(),
            value: serde_json::json!({
                "language": self.language,
                "script_length": self.script.len(),
            }),
        });

        // Placeholder: integrate rhai_rust or similar scripting runtime.
        tracing::info!(
            node_id = %self.node_id,
            "ScriptTask placeholder execution completed"
        );

        Ok(HandlerResult::Messages(vec![message]))
    }
}

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

/// Enhanced human task with form schema, assignment, and deadline.
///
/// First call: yields the form JSON as output and requests halt.
/// On resume (second call): passes through the approval result.
pub struct UserTask {
    pub node_id: String,
    pub form_schema: serde_json::Value,
    pub assignee: Option<String>,
    pub deadline: Option<Duration>,
    awaiting: Mutex<bool>,
}

impl UserTask {
    pub fn new(node_id: impl Into<String>, form_schema: serde_json::Value) -> Self {
        Self {
            node_id: node_id.into(),
            form_schema,
            assignee: None,
            deadline: None,
            awaiting: Mutex::new(false),
        }
    }

    pub fn with_assignee(mut self, assignee: impl Into<String>) -> Self {
        self.assignee = Some(assignee.into());
        self
    }

    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

#[async_trait]
impl IExecutor for UserTask {
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
            // Resume: pass through the approval result from external input
            *self.awaiting.lock() = false;

            if let Ok(approval) = Arc::downcast::<serde_json::Value>(message.clone()) {
                tracing::info!(
                    node_id = %self.node_id,
                    "UserTask resumed with approval result"
                );
                return Ok(HandlerResult::Messages(vec![approval]));
            }
            if let Ok(text) = Arc::downcast::<String>(message) {
                let val = serde_json::Value::String((*text).clone());
                return Ok(HandlerResult::Messages(vec![Arc::new(val)]));
            }

            return Ok(HandlerResult::None);
        }

        // First call: build the task form and halt for human input
        let mut form = self.form_schema.clone();
        if let Some(ref assignee) = self.assignee {
            if let serde_json::Value::Object(ref mut map) = form {
                map.insert("assignee".to_string(), serde_json::Value::String(assignee.clone()));
            }
        }
        if let Some(ref deadline) = self.deadline {
            if let serde_json::Value::Object(ref mut map) = form {
                map.insert(
                    "deadline_secs".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(deadline.as_secs())),
                );
            }
        }

        tracing::info!(
            node_id = %self.node_id,
            assignee = ?self.assignee,
            deadline = ?self.deadline,
            "UserTask yielding form and halting for human input"
        );

        let form_arc: Arc<dyn std::any::Any + Send + Sync> = Arc::new(form.clone());
        ctx.yield_output(form_arc).await?;
        ctx.request_halt_with_payload(form).await;
        *self.awaiting.lock() = true;

        Ok(HandlerResult::None)
    }
}

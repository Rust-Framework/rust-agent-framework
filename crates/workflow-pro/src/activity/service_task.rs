use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

/// Calls an external HTTP service/API.
///
/// On handle(), logs the request details via `tracing::info!` and passes
/// the incoming message downstream as `HandlerResult::Messages`.
pub struct ServiceTask {
    pub node_id: String,
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    pub timeout_ms: u64,
}

impl ServiceTask {
    pub fn new(
        node_id: impl Into<String>,
        url: impl Into<String>,
        method: impl Into<String>,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            url: url.into(),
            method: method.into(),
            headers: HashMap::new(),
            timeout_ms: 30_000,
        }
    }

    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

#[async_trait]
impl IExecutor for ServiceTask {
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
            url = %self.url,
            method = %self.method,
            timeout_ms = %self.timeout_ms,
            headers_count = %self.headers.len(),
            "ServiceTask invoking external HTTP service"
        );

        // Emit progress event indicating external call
        let _ = progress.send(NodeProgress::Custom {
            key: "service_task.invoking".into(),
            value: serde_json::json!({
                "url": self.url,
                "method": self.method,
                "timeout_ms": self.timeout_ms,
            }),
        });

        // Placeholder: in production, this would use reqwest/hyper to make
        // the actual HTTP call and deserialize the response.
        tracing::info!(
            node_id = %self.node_id,
            "ServiceTask HTTP call placeholder completed"
        );

        Ok(HandlerResult::Messages(vec![message]))
    }
}


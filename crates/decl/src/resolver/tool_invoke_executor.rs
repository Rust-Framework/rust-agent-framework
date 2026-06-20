//! InvokeFunctionTool 工作流执行器 — 通过 ToolResolver 真实调用 ITool。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{Result, ITool};
use rust_agent_workflow::engine::IWorkflowContext;
use rust_agent_workflow::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use tokio::sync::mpsc::UnboundedSender;

pub struct ToolInvokeExecutor {
    id: String,
    tool: Arc<dyn ITool>,
    arguments: HashMap<String, serde_json::Value>,
    output_variable: Option<String>,
}

impl ToolInvokeExecutor {
    pub fn new(
        id: impl Into<String>,
        tool: Arc<dyn ITool>,
        arguments: HashMap<String, serde_json::Value>,
        output_variable: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            tool,
            arguments,
            output_variable,
        }
    }
}

#[async_trait]
impl IExecutor for ToolInvokeExecutor {
    fn id(&self) -> &str {
        &self.id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("ChatMessage")]
    }

    fn send_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("String")]
    }

    fn is_output(&self) -> bool {
        true
    }

    async fn handle(
        &self,
        _message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let result = self.tool.execute(serde_json::json!(self.arguments)).await?;
        let text = if result.ok {
            result
                .data
                .map(|d| d.to_string())
                .unwrap_or_else(|| "ok".to_string())
        } else {
            result.error.unwrap_or_else(|| "tool error".to_string())
        };

        let _ = progress.send(NodeProgress::TextDelta(text.clone()));

        if let Some(ref key) = self.output_variable {
            ctx.write_state(key, serde_json::json!(text)).await?;
        }
        ctx.write_state("__tool_result", serde_json::json!(text))
            .await?;

        Ok(HandlerResult::Messages(vec![Arc::new(text)]))
    }
}

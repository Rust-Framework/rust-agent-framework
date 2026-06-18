use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;

use super::base::{HandlerResult, IExecutor, NodeProgress};
use crate::engine::WorkflowEngine;
use crate::engine::IWorkflowContext;
use crate::graph::WorkflowGraph;

/// 动态子流程执行器 — 运行时构造子图并执行
pub struct SubFlowExecutor {
    id: String,
    flow_factory: Arc<dyn Fn(&dyn IWorkflowContext) -> WorkflowGraph + Send + Sync>,
}

impl SubFlowExecutor {
    pub fn new(
        id: impl Into<String>,
        flow_factory: Arc<dyn Fn(&dyn IWorkflowContext) -> WorkflowGraph + Send + Sync>,
    ) -> Self {
        Self {
            id: id.into(),
            flow_factory,
        }
    }
}

#[async_trait]
impl IExecutor for SubFlowExecutor {
    fn id(&self) -> &str {
        &self.id
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        _progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let sub_graph = (self.flow_factory)(&*ctx);
        let engine = WorkflowEngine::new(sub_graph);
        let session = ctx.session().cloned();

        let (_events, mut outputs) = engine.run(message, session).await?;

        let mut produced: Vec<Arc<dyn std::any::Any + Send + Sync>> = Vec::new();
        while let Some(output_result) = outputs.next().await {
            match output_result {
                Ok(output) => produced.push(output.content),
                Err(e) => return Err(e),
            }
        }

        if produced.is_empty() {
            Ok(HandlerResult::None)
        } else {
            Ok(HandlerResult::Messages(produced))
        }
    }
}

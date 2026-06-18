use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;

use crate::engine::IWorkflowContext;
use crate::executor::base::{HandlerResult, IExecutor, NodeProgress};

/// 带工作流上下文的函数执行器 —— 在 `handle()` 中可访问 `Arc<dyn IWorkflowContext>`。
///
/// 闭包签名: `Fn(Arc<dyn Any>, Arc<dyn IWorkflowContext>, UnboundedSender<NodeProgress>) -> Fut`
pub struct ContextFunctionExecutor<F, Fut> {
    id: String,
    handler: F,
    _phantom: PhantomData<fn() -> Fut>,
}

impl<F, Fut> ContextFunctionExecutor<F, Fut>
where
    F: Fn(
            Arc<dyn std::any::Any + Send + Sync>,
            Arc<dyn IWorkflowContext>,
            UnboundedSender<NodeProgress>,
        ) -> Fut
        + Send
        + Sync
        + 'static,
    Fut: std::future::Future<Output = Result<HandlerResult>> + Send,
{
    pub fn new(id: impl Into<String>, handler: F) -> Self {
        Self { id: id.into(), handler, _phantom: PhantomData }
    }
}

#[async_trait]
impl<F, Fut> IExecutor for ContextFunctionExecutor<F, Fut>
where
    F: Fn(
            Arc<dyn std::any::Any + Send + Sync>,
            Arc<dyn IWorkflowContext>,
            UnboundedSender<NodeProgress>,
        ) -> Fut
        + Send
        + Sync
        + 'static,
    Fut: std::future::Future<Output = Result<HandlerResult>> + Send,
{
    fn id(&self) -> &str { &self.id }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        (self.handler)(message, ctx, progress).await
    }
}

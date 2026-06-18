use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;

use super::base::IExecutor;
use crate::engine::IWorkflowContext;

/// 可补偿执行器 — 节点失败时执行逆操作（Saga 模式）
#[async_trait]
pub trait ICompensable: IExecutor {
    /// 补偿操作
    async fn compensate_action(&self, ctx: Arc<dyn IWorkflowContext>) -> Result<()> {
        let _ = ctx;
        Ok(())
    }
}

/// 将任意补偿函数包装为 IExecutor
pub struct CompensableExecutor<E, F> {
    inner: E,
    compensate_fn: F,
}

impl<E, F> CompensableExecutor<E, F> {
    pub fn new(inner: E, compensate_fn: F) -> Self {
        Self { inner, compensate_fn }
    }

    pub fn inner(&self) -> &E {
        &self.inner
    }
}

#[async_trait]
impl<E, F, Fut> IExecutor for CompensableExecutor<E, F>
where
    E: IExecutor + Send + Sync,
    F: Fn(Arc<dyn IWorkflowContext>) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn accepted_types(&self) -> Vec<super::base::TypeTag> {
        self.inner.accepted_types()
    }

    fn send_types(&self) -> Vec<super::base::TypeTag> {
        self.inner.send_types()
    }

    fn is_output(&self) -> bool {
        self.inner.is_output()
    }

    fn as_agent(&self) -> Option<&Arc<dyn rust_agent_core::IAgent>> {
        self.inner.as_agent()
    }

    async fn on_init(&self, ctx: &dyn IWorkflowContext) -> Result<()> {
        self.inner.on_init(ctx).await
    }

    async fn on_checkpoint_save(&self, ctx: &dyn IWorkflowContext) -> Result<()> {
        self.inner.on_checkpoint_save(ctx).await
    }

    async fn on_checkpoint_restore(&self, ctx: &dyn IWorkflowContext) -> Result<()> {
        self.inner.on_checkpoint_restore(ctx).await
    }

    async fn on_delivery_start(&self, ctx: &dyn IWorkflowContext) -> Result<()> {
        self.inner.on_delivery_start(ctx).await
    }

    async fn on_delivery_end(&self, ctx: &dyn IWorkflowContext) -> Result<()> {
        self.inner.on_delivery_end(ctx).await
    }

    async fn on_timer(&self, timer_name: &str, ctx: &dyn IWorkflowContext) -> Result<()> {
        self.inner.on_timer(timer_name, ctx).await
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: tokio::sync::mpsc::UnboundedSender<super::base::NodeProgress>,
    ) -> Result<super::base::HandlerResult> {
        self.inner.handle(message, ctx, progress).await
    }

    async fn compensate(&self, ctx: Arc<dyn IWorkflowContext>) -> Result<()> {
        (self.compensate_fn)(ctx).await
    }
}

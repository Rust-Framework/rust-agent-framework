use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;

use super::base::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use crate::engine::IWorkflowContext;

/// 函数驱动的轻量 Executor — 对应 MAF 的 FunctionExecutor
pub struct FunctionExecutor<F, I, O> {
    id: String,
    handler: F,
    _phantom: PhantomData<(I, O)>,
}

impl<F, I, O> FunctionExecutor<F, I, O>
where
    F: Fn(I) -> O + Send + Sync + 'static,
    I: Send + Sync + 'static,
    O: Into<HandlerResult> + Send + Sync + 'static,
{
    pub fn new(id: impl Into<String>, handler: F) -> Self {
        Self {
            id: id.into(),
            handler,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<F, I, O> IExecutor for FunctionExecutor<F, I, O>
where
    F: Fn(I) -> O + Send + Sync + 'static,
    I: Clone + Send + Sync + 'static,
    O: Into<HandlerResult> + Send + Sync + 'static,
{
    fn id(&self) -> &str {
        &self.id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new(std::any::type_name::<I>())]
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        _ctx: Arc<dyn IWorkflowContext>,
        _progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        // Arc::downcast 返回 Result<Arc<I>, Arc<dyn Any>>
        let input: Arc<I> = message
            .downcast::<I>()
            .map_err(|_| {
                rust_agent_core::AgentError::WorkflowError(format!(
                    "类型不匹配: 期望 {}",
                    std::any::type_name::<I>()
                ))
            })?;

        let output = (self.handler)((*input).clone());
        Ok(output.into())
    }
}

// ── HandlerResult 构造辅助 ──

impl From<()> for HandlerResult {
    fn from(_: ()) -> Self {
        HandlerResult::None
    }
}

impl<T: Send + Sync + 'static> From<Vec<T>> for HandlerResult {
    fn from(items: Vec<T>) -> Self {
        let arcs: Vec<Arc<dyn std::any::Any + Send + Sync>> = items
            .into_iter()
            .map(|item| Arc::new(item) as Arc<dyn std::any::Any + Send + Sync>)
            .collect();
        HandlerResult::Messages(arcs)
    }
}

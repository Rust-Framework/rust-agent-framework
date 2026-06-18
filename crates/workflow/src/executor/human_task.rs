use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;

use super::base::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use crate::engine::IWorkflowContext;

/// 人工任务执行器 — 暂停工作流等待外部审批
///
/// 首次调用：构造审批表单 → yield_output → request_halt
/// 恢复后：收到注入的审批结果 → 返回给下游
pub struct HumanTaskExecutor {
    id: String,
    task_builder: Arc<dyn Fn(&dyn IWorkflowContext) -> serde_json::Value + Send + Sync>,
    awaiting: Mutex<bool>,
}

impl HumanTaskExecutor {
    pub fn new(
        id: impl Into<String>,
        task_builder: Arc<dyn Fn(&dyn IWorkflowContext) -> serde_json::Value + Send + Sync>,
    ) -> Self {
        Self {
            id: id.into(),
            awaiting: Mutex::new(false),
            task_builder,
        }
    }
}

#[async_trait]
impl IExecutor for HumanTaskExecutor {
    fn id(&self) -> &str {
        &self.id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![
            TypeTag::new("initial"),
            TypeTag::new("resume"),
            TypeTag::new(std::any::type_name::<String>()),
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
            *self.awaiting.lock() = false;

            if let Ok(approval) = Arc::downcast::<serde_json::Value>(message.clone()) {
                return Ok(HandlerResult::Messages(vec![approval]));
            }
            if let Ok(text) = Arc::downcast::<String>(message) {
                let val = serde_json::Value::String((*text).clone());
                return Ok(HandlerResult::Messages(vec![Arc::new(val)]));
            }

            return Ok(HandlerResult::None);
        }

        let form = (self.task_builder)(&*ctx);
        let form_arc: Arc<dyn std::any::Any + Send + Sync> = Arc::new(form.clone());
        ctx.yield_output(form_arc).await?;
        ctx.request_halt_with_payload(form).await;
        *self.awaiting.lock() = true;

        Ok(HandlerResult::None)
    }
}

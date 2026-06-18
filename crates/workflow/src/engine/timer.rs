use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;

use crate::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use crate::engine::IWorkflowContext;

/// 延迟触发执行器 —— 等待指定时间后将消息路由到下游节点。
///
/// 作为 IExecutor 插入工作流图中，引擎 SuperStep 循环中的
/// `fire_due_timers()` 在 timer 到期后自动 re-enqueue 消息到此节点，
/// 此时 handle() 将消息原样传递给下游。
pub struct TimerTrigger {
    node_id: String,
    delay: Duration,
    timer_name: String,
    fired: Mutex<bool>,
}

impl TimerTrigger {
    /// 创建一个延迟触发器。
    ///
    /// `node_id` — 图中节点 ID
    /// `delay` — 等待时长
    pub fn new(node_id: impl Into<String>, delay: Duration) -> Self {
        let id: String = node_id.into();
        Self {
            timer_name: format!("timer_{}", id),
            node_id: id,
            delay,
            fired: Mutex::new(false),
        }
    }

    /// 以指定名称创建延迟触发器
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.timer_name = name.into();
        self
    }
}

#[async_trait]
impl IExecutor for TimerTrigger {
    fn id(&self) -> &str {
        &self.node_id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![
            TypeTag::new("initial"),
            TypeTag::new("timer"),
            TypeTag::new(std::any::type_name::<String>()),
        ]
    }

    async fn on_timer(&self, timer_name: &str, _ctx: &dyn IWorkflowContext) -> Result<()> {
        if timer_name == self.timer_name {
            *self.fired.lock() = true;
        }
        Ok(())
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let already_fired = *self.fired.lock();

        if already_fired {
            // Timer 已触发 —— 将消息原样传递给下游
            *self.fired.lock() = false;

            let _ = progress.send(NodeProgress::Custom {
                key: "timer_fired".into(),
                value: serde_json::json!({
                    "node_id": self.node_id,
                    "timer_name": self.timer_name,
                    "delay_ms": self.delay.as_millis(),
                }),
            });

            Ok(HandlerResult::Messages(vec![message]))
        } else {
            // 首次调用 —— 注册定时器，进入等待
            ctx.schedule_timer(&self.timer_name, self.delay).await?;

            let _ = progress.send(NodeProgress::Custom {
                key: "timer_scheduled".into(),
                value: serde_json::json!({
                    "node_id": self.node_id,
                    "timer_name": self.timer_name,
                    "delay_ms": self.delay.as_millis(),
                }),
            });

            Ok(HandlerResult::None)
        }
    }
}

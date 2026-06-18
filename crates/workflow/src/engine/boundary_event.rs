use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use crate::engine::IWorkflowContext;
use crate::graph::edge::IEdgeCondition;

/// 边界事件类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BoundaryEventKind {
    /// 定时器边界事件（超时触发）
    Timer(Duration),
    /// 错误边界事件（错误码匹配）
    Error(String),
    /// 信号边界事件（信号名称匹配）
    Signal(String),
    /// 消息边界事件（关联键匹配）
    Message(String),
    /// 升级边界事件
    Escalation(String),
    /// 补偿边界事件
    Compensation,
}

/// 中间事件类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntermediateEventKind {
    /// Catch — 等待事件发生
    Catch,
    /// Throw — 主动触发事件
    Throw,
}

/// 事件定义 — 与 BoundaryEvent 或 IntermediateEvent 绑定的具体事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventDefinition {
    Timer { duration: Option<Duration> },
    Signal { name: String },
    Message { name: String },
    Error { code: String },
    Escalation { code: String },
    Compensation { activity_id: Option<String> },
    Link { name: String },
}

/// 边界事件 —— 附加在节点上的中断或非中断事件。
///
/// 对应 BPMN 的 BoundaryEvent：
/// - **中断（interrupting=true）**：事件触发时取消原节点执行，沿边界事件分支继续
/// - **非中断（interrupting=false）**：事件触发时并行启动新分支，原节点继续执行
#[derive(Debug, Clone)]
pub struct BoundaryEvent {
    /// 附加的节点 ID
    pub attached_to: String,
    /// 事件类型
    pub kind: BoundaryEventKind,
    /// 是否中断原节点
    pub interrupting: bool,
    /// 此边界事件自身的节点 ID
    pub event_node_id: String,
}

impl BoundaryEvent {
    pub fn new(attached_to: impl Into<String>, kind: BoundaryEventKind, event_node_id: impl Into<String>) -> Self {
        Self {
            attached_to: attached_to.into(),
            kind,
            interrupting: true,
            event_node_id: event_node_id.into(),
        }
    }

    /// 设置为非中断边界事件。
    pub fn non_interrupting(mut self) -> Self {
        self.interrupting = false;
        self
    }

    /// 创建定时器边界事件。
    pub fn timer(attached_to: impl Into<String>, timeout: Duration, event_node_id: impl Into<String>) -> Self {
        Self::new(attached_to, BoundaryEventKind::Timer(timeout), event_node_id)
    }

    /// 创建错误边界事件。
    pub fn error(attached_to: impl Into<String>, error_code: impl Into<String>, event_node_id: impl Into<String>) -> Self {
        Self::new(attached_to, BoundaryEventKind::Error(error_code.into()), event_node_id)
    }

    /// 创建信号边界事件。
    pub fn signal(attached_to: impl Into<String>, signal_name: impl Into<String>, event_node_id: impl Into<String>) -> Self {
        Self::new(attached_to, BoundaryEventKind::Signal(signal_name.into()), event_node_id)
    }
}

/// 中间事件节点执行器 —— 等待（Catch）或触发（Throw）事件。
///
/// 作为 IExecutor 插入工作流图中：
/// - Catch 模式：暂停直到收到匹配的事件，然后继续下游
/// - Throw 模式：触发事件后立即继续下游
pub struct IntermediateEvent {
    node_id: String,
    event_def: EventDefinition,
    kind: IntermediateEventKind,
    fired: parking_lot::Mutex<bool>,
}

impl IntermediateEvent {
    pub fn new(node_id: impl Into<String>, event_def: EventDefinition, kind: IntermediateEventKind) -> Self {
        Self {
            node_id: node_id.into(),
            event_def,
            kind,
            fired: parking_lot::Mutex::new(false),
        }
    }

    /// 创建一个 catch 型中间事件。
    pub fn catch(node_id: impl Into<String>, event_def: EventDefinition) -> Self {
        Self::new(node_id, event_def, IntermediateEventKind::Catch)
    }

    /// 创建一个 throw 型中间事件。
    pub fn throw(node_id: impl Into<String>, event_def: EventDefinition) -> Self {
        Self::new(node_id, event_def, IntermediateEventKind::Throw)
    }
}

#[async_trait]
impl IExecutor for IntermediateEvent {
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

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        match self.kind {
            IntermediateEventKind::Throw => {
                let _ = progress.send(NodeProgress::Custom {
                    key: "event_thrown".into(),
                    value: serde_json::json!({
                        "node_id": self.node_id,
                        "event": self.event_def,
                    }),
                });
                Ok(HandlerResult::Messages(vec![message]))
            }
            IntermediateEventKind::Catch => {
                let already_fired = *self.fired.lock();

                if already_fired {
                    *self.fired.lock() = false;
                    let _ = progress.send(NodeProgress::Custom {
                        key: "event_caught".into(),
                        value: serde_json::json!({
                            "node_id": self.node_id,
                            "event": self.event_def,
                        }),
                    });
                    return Ok(HandlerResult::Messages(vec![message]));
                }

                // 根据事件类型注册等待
                match &self.event_def {
                    EventDefinition::Timer { duration } => {
                        if let Some(delay) = duration {
                            ctx.schedule_timer(&self.node_id, *delay).await?;
                        }
                    }
                    EventDefinition::Signal { .. }
                    | EventDefinition::Message { .. }
                    | EventDefinition::Error { .. } => {
                        // 等待外部注入
                        ctx.request_halt().await;
                    }
                    _ => {}
                }

                let _ = progress.send(NodeProgress::Custom {
                    key: "event_waiting".into(),
                    value: serde_json::json!({
                        "node_id": self.node_id,
                        "event": self.event_def,
                    }),
                });

                Ok(HandlerResult::None)
            }
        }
    }

    async fn on_timer(&self, timer_name: &str, _ctx: &dyn IWorkflowContext) -> Result<()> {
        if timer_name == self.node_id {
            *self.fired.lock() = true;
        }
        Ok(())
    }
}

/// 边界事件条件 — 作为 IEdgeCondition 判断是否需要沿事件分支路由。
///
/// 附加在从原节点到事件处理器节点的边上。
pub struct BoundaryEventCondition {
    pub event: BoundaryEvent,
}

impl BoundaryEventCondition {
    pub fn new(event: BoundaryEvent) -> Self {
        Self { event }
    }
}

#[async_trait]
impl IEdgeCondition for BoundaryEventCondition {
    async fn evaluate(&self, envelope: &crate::engine::message_envelope::MessageEnvelope) -> Result<bool> {
        let meta = &envelope.metadata;

        let matched = match &self.event.kind {
            BoundaryEventKind::Error(code) => {
                meta.get("error_code").and_then(|v| v.as_str()) == Some(code.as_str())
            }
            BoundaryEventKind::Signal(name) => {
                meta.get("signal_name").and_then(|v| v.as_str()) == Some(name.as_str())
            }
            BoundaryEventKind::Message(name) => {
                meta.get("message_name").and_then(|v| v.as_str()) == Some(name.as_str())
            }
            BoundaryEventKind::Timer(_) => {
                meta.get("boundary_timer_fired").and_then(|v| v.as_str()) == Some(&self.event.event_node_id)
            }
            _ => false,
        };

        Ok(matched)
    }
}

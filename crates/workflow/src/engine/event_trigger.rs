use serde::Serialize;
use std::sync::Arc;

/// 外部事件 — 通过 `WorkflowEngine::inject_event()` 注入工作流。
///
/// 支持三种事件类型，映射到不同的节点触发机制：
/// - `MessageReceived`: 通过 RequestPort 映射到目标节点
/// - `SignalReceived`: 广播到所有监听该信号的节点
/// - `TimerElapsed`: 触发对应定时器的 on_timer 钩子
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event_kind", content = "data")]
pub enum ExternalEvent {
    /// 收到外部消息，路由到指定端口的节点
    MessageReceived {
        /// 目标 RequestPort ID
        port_id: String,
        /// 消息体（JSON）
        payload: serde_json::Value,
    },
    /// 收到外部信号，广播到所有等待该信号的节点
    SignalReceived {
        /// 信号名称
        signal_name: String,
        /// 信号附带数据
        payload: serde_json::Value,
    },
    /// 定时器到期
    TimerElapsed {
        /// 定时器 ID
        timer_id: String,
    },
}

impl ExternalEvent {
    /// 创建消息接收事件
    pub fn message(port_id: impl Into<String>, payload: serde_json::Value) -> Self {
        Self::MessageReceived {
            port_id: port_id.into(),
            payload,
        }
    }

    /// 创建信号接收事件
    pub fn signal(signal_name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self::SignalReceived {
            signal_name: signal_name.into(),
            payload,
        }
    }

    /// 创建定时器到期事件
    pub fn timer(timer_id: impl Into<String>) -> Self {
        Self::TimerElapsed {
            timer_id: timer_id.into(),
        }
    }
}

/// 事件总线 —— 用于管理 ExternalEvent 的接收和分发。
///
/// 使用 tokio broadcast channel 实现多播，
/// 支持多个订阅者同时监听外部事件。
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<Arc<ExternalEvent>>,
}

impl EventBus {
    /// 创建新的事件总线，capacity 为缓冲区大小。
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    /// 发布事件到所有订阅者。
    pub fn publish(&self, event: ExternalEvent) -> Result<(), tokio::sync::broadcast::error::SendError<Arc<ExternalEvent>>> {
        self.tx.send(Arc::new(event)).map(|_| ())
    }

    /// 订阅事件流。
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Arc<ExternalEvent>> {
        self.tx.subscribe()
    }

    /// 获取发送器引用，供引擎内部使用。
    pub fn sender(&self) -> tokio::sync::broadcast::Sender<Arc<ExternalEvent>> {
        self.tx.clone()
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus").finish()
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

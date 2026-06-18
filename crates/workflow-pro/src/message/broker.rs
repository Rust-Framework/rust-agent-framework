use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 消息代理抽象 —— 外部消息中间件（Kafka、RabbitMQ、Redis 等）的集成接口。
///
/// workflow-pro 通过此 trait 与外部消息系统交互，
/// SendTask 调用 `publish()`，ReceiveTask 通过 `MessageCorrelation` 等待消息。
#[async_trait]
pub trait IMessageBroker: Send + Sync {
    /// 发布消息到指定主题/队列。
    async fn publish(
        &self,
        topic: &str,
        payload: &serde_json::Value,
        headers: Option<HashMap<String, String>>,
    ) -> Result<(), String>;

    /// 订阅指定主题/队列的消息。
    /// 返回一个接收器，用于异步接收消息。
    async fn subscribe(
        &self,
        topic: &str,
    ) -> Result<Box<dyn MessageReceiver>, String>;

    /// 发送请求并等待回复（RPC 模式）。
    async fn request(
        &self,
        topic: &str,
        payload: &serde_json::Value,
        timeout_ms: u64,
    ) -> Result<serde_json::Value, String>;

    /// 检查消息代理健康状态。
    async fn health_check(&self) -> bool;
}

/// 消息接收器 —— 从消息代理接收消息的流式接口。
#[async_trait]
pub trait MessageReceiver: Send + Sync {
    /// 接收下一条消息（阻塞直到有消息或超时）。
    async fn recv(&mut self, timeout_ms: u64) -> Result<Option<ReceivedMessage>, String>;
}

/// 接收到的消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceivedMessage {
    pub topic: String,
    pub payload: serde_json::Value,
    pub headers: HashMap<String, String>,
    pub message_id: Option<String>,
    pub timestamp: Option<i64>,
}

/// 内存消息代理（用于测试和简单场景）。
pub struct InMemoryMessageBroker {
    /// topic → queue of messages
    queues: parking_lot::Mutex<HashMap<String, Vec<ReceivedMessage>>>,
}

impl InMemoryMessageBroker {
    pub fn new() -> Self {
        Self {
            queues: parking_lot::Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryMessageBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IMessageBroker for InMemoryMessageBroker {
    async fn publish(
        &self,
        topic: &str,
        payload: &serde_json::Value,
        headers: Option<HashMap<String, String>>,
    ) -> Result<(), String> {
        let msg = ReceivedMessage {
            topic: topic.to_string(),
            payload: payload.clone(),
            headers: headers.unwrap_or_default(),
            message_id: Some(uuid::Uuid::new_v4().to_string()),
            timestamp: Some(chrono::Utc::now().timestamp_millis()),
        };

        let mut queues = self.queues.lock();
        queues.entry(topic.to_string()).or_default().push(msg);
        Ok(())
    }

    async fn subscribe(
        &self,
        topic: &str,
    ) -> Result<Box<dyn MessageReceiver>, String> {
        let queues = self.queues.lock();
        let messages = queues.get(topic).cloned().unwrap_or_default();
        Ok(Box::new(InMemoryReceiver {
            messages,
            index: 0,
        }))
    }

    async fn request(
        &self,
        topic: &str,
        payload: &serde_json::Value,
        _timeout_ms: u64,
    ) -> Result<serde_json::Value, String> {
        self.publish(topic, payload, None).await?;
        Ok(serde_json::json!({"status": "published"}))
    }

    async fn health_check(&self) -> bool {
        true
    }
}

struct InMemoryReceiver {
    messages: Vec<ReceivedMessage>,
    index: usize,
}

#[async_trait]
impl MessageReceiver for InMemoryReceiver {
    async fn recv(&mut self, _timeout_ms: u64) -> Result<Option<ReceivedMessage>, String> {
        if self.index < self.messages.len() {
            let msg = self.messages[self.index].clone();
            self.index += 1;
            Ok(Some(msg))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_broker_publish_subscribe() {
        let broker = InMemoryMessageBroker::new();

        broker.publish("test.topic", &serde_json::json!({"msg": "hello"}), None)
            .await
            .unwrap();

        let mut receiver = broker.subscribe("test.topic").await.unwrap();
        let msg = receiver.recv(1000).await.unwrap();
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().payload["msg"], "hello");
    }

    #[tokio::test]
    async fn test_broker_health_check() {
        let broker = InMemoryMessageBroker::new();
        assert!(broker.health_check().await);
    }
}

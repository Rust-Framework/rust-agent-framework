use std::collections::{HashMap, VecDeque};

use crate::checkpoint::SerializableMessageEnvelope;

use super::message_envelope::MessageEnvelope;

/// 单步消息上下文
#[derive(Debug, Default)]
pub struct StepContext {
    queued_messages: HashMap<String, VecDeque<MessageEnvelope>>,
    pub step_number: i32,
}

impl StepContext {
    pub fn new(step_number: i32) -> Self {
        Self {
            queued_messages: HashMap::new(),
            step_number,
        }
    }

    pub fn enqueue(&mut self, envelope: MessageEnvelope) {
        let target_id = envelope
            .target_node_id
            .clone()
            .unwrap_or_else(|| "broadcast".to_string());
        self.queued_messages
            .entry(target_id)
            .or_default()
            .push_back(envelope);
    }

    pub fn enqueue_batch<I: IntoIterator<Item = MessageEnvelope>>(&mut self, envelopes: I) {
        for envelope in envelopes {
            self.enqueue(envelope);
        }
    }

    pub fn dequeue_for(&mut self, node_id: &str) -> Option<VecDeque<MessageEnvelope>> {
        self.queued_messages.remove(node_id)
    }

    pub fn has_messages(&self) -> bool {
        !self.queued_messages.is_empty()
    }

    pub fn active_nodes(&self) -> Vec<String> {
        self.queued_messages.keys().cloned().collect()
    }

    pub fn message_count(&self) -> usize {
        self.queued_messages.values().map(|q| q.len()).sum()
    }

    /// 序列化所有 pending messages 用于 checkpoint
    pub fn serialize_pending(&self) -> Vec<SerializableMessageEnvelope> {
        let mut result = Vec::new();
        for queue in self.queued_messages.values() {
            for env in queue {
                result.push(env.to_serializable());
            }
        }
        result
    }

    /// 从 checkpoint 还原的 pending messages 重建 StepContext
    /// 保持原有的 step_number
    pub fn from_serialized(
        step_number: i32,
        serializables: Vec<SerializableMessageEnvelope>,
    ) -> rust_agent_core::Result<Self> {
        let mut ctx = Self::new(step_number);
        use crate::checkpoint::deserialize_envelopes;
        let envelopes = deserialize_envelopes(serializables)?;
        for env in envelopes {
            ctx.enqueue(env);
        }
        Ok(ctx)
    }
}

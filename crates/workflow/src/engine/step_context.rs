use std::collections::{HashMap, VecDeque};

use super::message_envelope::MessageEnvelope;

/// 单步消息上下文 — 对应 MAF 的 StepContext
///
/// 每个 SuperStep 中排队待投递的消息，按目标节点 ID 分组。
#[derive(Debug, Default)]
pub struct StepContext {
    /// 按目标节点 ID 分组的消息队列
    queued_messages: HashMap<String, VecDeque<MessageEnvelope>>,
    /// 当前步骤号
    pub step_number: i32,
}

impl StepContext {
    pub fn new(step_number: i32) -> Self {
        Self {
            queued_messages: HashMap::new(),
            step_number,
        }
    }

    /// 入队一条消息到指定目标节点
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

    /// 批量入队消息
    pub fn enqueue_batch<I: IntoIterator<Item = MessageEnvelope>>(&mut self, envelopes: I) {
        for envelope in envelopes {
            self.enqueue(envelope);
        }
    }

    /// 获取某个节点的消息队列
    pub fn dequeue_for(&mut self, node_id: &str) -> Option<VecDeque<MessageEnvelope>> {
        self.queued_messages.remove(node_id)
    }

    /// 是否有未处理的消息
    pub fn has_messages(&self) -> bool {
        !self.queued_messages.is_empty()
    }

    /// 获取所有有消息的节点 ID
    pub fn active_nodes(&self) -> Vec<String> {
        self.queued_messages.keys().cloned().collect()
    }

    /// 消息总数
    pub fn message_count(&self) -> usize {
        self.queued_messages.values().map(|q| q.len()).sum()
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;

use crate::executor::IExecutor;
use crate::graph::edge::{DirectEdgeData, FanInEdgeData, FanOutEdgeData};

use super::message_envelope::MessageEnvelope;

/// 消息投递目标
#[derive(Debug)]
pub struct MessageDelivery {
    pub envelope: MessageEnvelope,
    pub target_node_id: String,
}

/// 边执行器 trait — 对应 MAF 的 EdgeRunner
///
/// 纯函数：根据消息信封决定投递到哪些目标节点。
#[async_trait]
pub trait IEdgeRunner: Send + Sync {
    /// 计算消息的投递目标
    async fn chase(
        &self,
        envelope: &MessageEnvelope,
        nodes: &HashMap<String, Arc<dyn IExecutor>>,
    ) -> Result<Vec<MessageDelivery>>;

    /// 导出边执行器内部状态以供 checkpoint 持久化（默认空）
    fn checkpoint_state(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }
}

// ── 直接边执行器 ──

pub struct DirectEdgeRunner {
    pub edge_data: DirectEdgeData,
}

#[async_trait]
impl IEdgeRunner for DirectEdgeRunner {
    async fn chase(
        &self,
        envelope: &MessageEnvelope,
        nodes: &HashMap<String, Arc<dyn IExecutor>>,
    ) -> Result<Vec<MessageDelivery>> {
        // 检查条件
        if let Some(ref condition) = self.edge_data.condition {
            if !condition.evaluate(envelope).await? {
                return Ok(vec![]);
            }
        }

        // 检查目标节点是否存在
        if !nodes.contains_key(&self.edge_data.sink_id) {
            return Ok(vec![]);
        }

        let delivery = MessageDelivery {
            envelope: envelope.clone(),
            target_node_id: self.edge_data.sink_id.clone(),
        };

        Ok(vec![delivery])
    }
}

// ── 扇出边执行器 ──

pub struct FanOutEdgeRunner {
    pub edge_data: FanOutEdgeData,
}

#[async_trait]
impl IEdgeRunner for FanOutEdgeRunner {
    async fn chase(
        &self,
        envelope: &MessageEnvelope,
        nodes: &HashMap<String, Arc<dyn IExecutor>>,
    ) -> Result<Vec<MessageDelivery>> {
        // 确定目标节点
        let targets: Vec<String> = if let Some(ref assigner) = self.edge_data.assigner {
            assigner.targets(envelope)
        } else {
            self.edge_data.sink_ids.clone()
        };

        let deliveries: Vec<MessageDelivery> = targets
            .into_iter()
            .filter(|tid| nodes.contains_key(tid))
            .map(|target_node_id| MessageDelivery {
                envelope: envelope.clone(),
                target_node_id,
            })
            .collect();

        Ok(deliveries)
    }
}

// ── 扇入边执行器（带栅栏状态） ──

use parking_lot::Mutex;

#[derive(Debug, Default)]
pub struct FanInState {
    /// 记录哪些源已到达，存储其消息
    pub received: HashMap<String, Vec<MessageEnvelope>>,
}

pub struct FanInEdgeRunner {
    pub edge_data: FanInEdgeData,
    pub state: Mutex<FanInState>,
}

impl FanInEdgeRunner {
    pub fn new(edge_data: FanInEdgeData) -> Self {
        Self {
            edge_data,
            state: Mutex::new(FanInState::default()),
        }
    }

    pub fn reset(&self) {
        let mut state = self.state.lock();
        state.received.clear();
    }
}

#[async_trait]
impl IEdgeRunner for FanInEdgeRunner {
    fn checkpoint_state(&self) -> HashMap<String, serde_json::Value> {
        let state = self.state.lock();
        let received_map: serde_json::Map<String, serde_json::Value> = state
            .received
            .iter()
            .map(|(k, v)| {
                let count = serde_json::Value::Number(serde_json::Number::from(v.len()));
                (k.clone(), count)
            })
            .collect();
        let mut map = HashMap::new();
        map.insert(
            format!("fanin_{}", self.edge_data.edge_id),
            serde_json::Value::Object(received_map),
        );
        map
    }

    async fn chase(
        &self,
        envelope: &MessageEnvelope,
        nodes: &HashMap<String, Arc<dyn IExecutor>>,
    ) -> Result<Vec<MessageDelivery>> {
        let source_id = envelope.source_node_id.clone();

        // 记录消息
        {
            let mut state = self.state.lock();
            state
                .received
                .entry(source_id)
                .or_default()
                .push(envelope.clone());
        }

        // 检查是否所有源都已到达
        let mut state = self.state.lock();
        let required_sources: Vec<&str> =
            self.edge_data.source_ids.iter().map(|s| s.as_str()).collect();

        if required_sources.iter().all(|sid| state.received.contains_key(*sid)) {
            // 栅栏就绪 — 收集所有源的全部消息，合并投递到目标
            if !nodes.contains_key(&self.edge_data.sink_id) {
                return Ok(vec![]);
            }

            let all_deliveries: Vec<MessageDelivery> = state
                .received
                .values()
                .flat_map(|envs| envs.iter())
                .map(|env| MessageDelivery {
                    envelope: env.clone(),
                    target_node_id: self.edge_data.sink_id.clone(),
                })
                .collect();

            // 重置栅栏，为下一轮消息做准备
            state.received.clear();

            Ok(all_deliveries)
        } else {
            // 栅栏未就绪
            Ok(vec![])
        }
    }
}

// ── 辅助函数 ──

/// 从边数据创建对应的 IEdgeRunner
pub fn create_edge_runner(
    edge: &crate::graph::Edge,
) -> Box<dyn IEdgeRunner> {
    match edge {
        crate::graph::Edge::Direct(data) => Box::new(DirectEdgeRunner {
            edge_data: data.clone(),
        }),
        crate::graph::Edge::FanOut(data) => Box::new(FanOutEdgeRunner {
            edge_data: data.clone(),
        }),
        crate::graph::Edge::FanIn(data) => Box::new(FanInEdgeRunner::new(data.clone())),
    }
}

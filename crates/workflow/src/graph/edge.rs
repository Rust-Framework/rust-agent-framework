use async_trait::async_trait;
use rust_agent_core::Result;
use serde::{Deserialize, Serialize};

use crate::engine::message_envelope::MessageEnvelope;

/// 边 ID — 基于字符串的唯一标识
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EdgeId(pub String);

impl EdgeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EdgeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── 边数据结构 ──

#[derive(Debug, Clone)]
pub struct DirectEdgeData {
    pub edge_id: EdgeId,
    pub source_id: String,
    pub sink_id: String,
    pub label: Option<String>,
    #[allow(clippy::type_complexity)]
    pub condition: Option<std::sync::Arc<dyn IEdgeCondition>>,
    /// 显式标记为循环回边 — 图校验允许此边形成环
    pub is_loopback: bool,
}

#[derive(Debug, Clone)]
pub struct FanOutEdgeData {
    pub edge_id: EdgeId,
    pub source_id: String,
    pub sink_ids: Vec<String>,
    pub label: Option<String>,
    #[allow(clippy::type_complexity)]
    pub assigner: Option<std::sync::Arc<dyn IFanOutAssigner>>,
}

#[derive(Debug, Clone)]
pub struct FanInEdgeData {
    pub edge_id: EdgeId,
    pub source_ids: Vec<String>,
    pub sink_id: String,
    pub label: Option<String>,
}

// ── 边枚举 ──

/// 工作流图中的边 — 标记联合体，Hash/Eq 仅基于 EdgeId
/// 对应 MAF 的 `Edge`（EdgeKind 枚举）
#[derive(Debug, Clone)]
pub enum Edge {
    Direct(DirectEdgeData),
    FanOut(FanOutEdgeData),
    FanIn(FanInEdgeData),
}

impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        self.edge_id() == other.edge_id()
    }
}

impl Eq for Edge {}

impl std::hash::Hash for Edge {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.edge_id().hash(state);
    }
}

impl Edge {
    pub fn edge_id(&self) -> &EdgeId {
        match self {
            Edge::Direct(d) => &d.edge_id,
            Edge::FanOut(d) => &d.edge_id,
            Edge::FanIn(d) => &d.edge_id,
        }
    }

    pub fn source_ids(&self) -> Vec<&str> {
        match self {
            Edge::Direct(d) => vec![d.source_id.as_str()],
            Edge::FanOut(d) => vec![d.source_id.as_str()],
            Edge::FanIn(d) => d.source_ids.iter().map(|s| s.as_str()).collect(),
        }
    }

    pub fn sink_ids(&self) -> Vec<&str> {
        match self {
            Edge::Direct(d) => vec![d.sink_id.as_str()],
            Edge::FanOut(d) => d.sink_ids.iter().map(|s| s.as_str()).collect(),
            Edge::FanIn(d) => vec![d.sink_id.as_str()],
        }
    }
}

// ── 边条件 / 分配器 trait ──

/// 直接边的条件过滤器 — 返回 false 则消息不沿此边传递
#[async_trait]
pub trait IEdgeCondition: Send + Sync {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> Result<bool>;
}

impl std::fmt::Debug for dyn IEdgeCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IEdgeCondition").finish()
    }
}

/// 扇出边的目标分配器 — 决定消息路由到哪些目标节点
pub trait IFanOutAssigner: Send + Sync {
    fn targets(&self, envelope: &MessageEnvelope) -> Vec<String>;
}

impl std::fmt::Debug for dyn IFanOutAssigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IFanOutAssigner").finish()
    }
}

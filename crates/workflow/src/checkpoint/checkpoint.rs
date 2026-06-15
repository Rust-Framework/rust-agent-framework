use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 检查点 — 工作流执行状态的完整快照
///
/// # 增量策略
///
/// - `parent_checkpoint_id = None` → 全量快照（初始检查点或压缩后的）
/// - `parent_checkpoint_id = Some(id)` → 增量快照，恢复时沿 parent 链回溯合并
///
/// # 故障恢复
///
/// 每个 SuperStep 结束后自动保存。恢复时从最新检查点加载，
/// 沿 parent 链回溯到最近的的全量快照，合并 state_data。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// 步骤编号（-1 = 初始检查点）
    pub step_number: i32,

    /// 工作流拓扑指纹 — 恢复时校验图结构未变
    pub graph_fingerprint: String,

    /// 用户状态数据：scope_key_string → serde_json::Value
    ///
    /// 存储格式：`"node_id"` 或 `"node_id::scope_name"`（通过 ScopeKey::to_string() 生成）
    pub state_data: HashMap<String, serde_json::Value>,

    /// 边状态数据：edge_key → serde_json::Value（FanIn 栅栏等）
    pub edge_state_data: HashMap<String, serde_json::Value>,

    /// 当前 StepContext 中未处理的消息
    pub pending_messages: Vec<super::message_envelope::SerializableMessageEnvelope>,

    /// 父检查点 ID — None = 全量快照
    pub parent_checkpoint_id: Option<String>,

    /// 是否为全量快照（显式标记，便于快速判断）
    pub is_full_snapshot: bool,

    /// 创建时间
    pub created_at: DateTime<Utc>,
}

/// 检查点元数据 — 轻量指针，不包含实际数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    pub checkpoint_id: String,
    pub session_id: String,
    pub step_number: i32,
    pub parent_checkpoint_id: Option<String>,
    pub is_full_snapshot: bool,
    /// 序列化后的字节大小（运维监控用）
    pub byte_size: u64,
    pub created_at: DateTime<Utc>,
}

/// 状态作用域键 — 按 (node_id, scope_name) 隔离
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopeKey {
    pub node_id: String,
    /// None = 私有作用域, Some(name) = 命名共享作用域
    pub scope_name: Option<String>,
}

impl ScopeKey {
    pub fn private(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            scope_name: None,
        }
    }

    pub fn shared(node_id: impl Into<String>, scope_name: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            scope_name: Some(scope_name.into()),
        }
    }

    /// 序列化为字符串键，用于 JSON HashMap key
    ///
    /// 私有作用域 → `"node_id"`
    /// 共享作用域 → `"node_id::scope_name"`
    pub fn to_key_string(&self) -> String {
        match &self.scope_name {
            Some(name) => format!("{}::{}", self.node_id, name),
            None => self.node_id.clone(),
        }
    }

    /// 从字符串键反序列化
    pub fn from_key_string(key: &str) -> Self {
        if let Some(pos) = key.find("::") {
            Self {
                node_id: key[..pos].to_string(),
                scope_name: Some(key[pos + 2..].to_string()),
            }
        } else {
            Self {
                node_id: key.to_string(),
                scope_name: None,
            }
        }
    }
}

/// 检查点配置
#[derive(Debug, Clone)]
pub struct CheckpointConfig {
    /// 每多少次增量后触发一次全量压缩（默认 50）
    pub full_snapshot_interval: u32,
    /// 最大保留检查点数（默认 100，超出后删除最旧的）
    pub max_checkpoints: usize,
    /// 是否启用检查点（默认 true）
    pub enabled: bool,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            full_snapshot_interval: 50,
            max_checkpoints: 100,
            enabled: true,
        }
    }
}

impl CheckpointConfig {
    pub fn with_interval(mut self, interval: u32) -> Self {
        self.full_snapshot_interval = interval;
        self
    }

    pub fn with_max_checkpoints(mut self, max: usize) -> Self {
        self.max_checkpoints = max;
        self
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

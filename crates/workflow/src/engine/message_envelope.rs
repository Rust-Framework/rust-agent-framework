use std::collections::HashMap;

use crate::executor::TypeTag;

/// 消息信封 — 在图中传递的消息，携带路由元数据
///
/// 对应 MAF 的 MessageEnvelope。
#[derive(Debug)]
pub struct MessageEnvelope {
    pub message_id: String,
    pub source_node_id: String,
    /// None 表示广播
    pub target_node_id: Option<String>,
    /// 注意：Box<dyn Any> 不可 Clone，MessageEnvelope 手动实现 Clone（content 占位）
    pub content: Box<dyn std::any::Any + Send + Sync>,
    pub type_tag: TypeTag,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl MessageEnvelope {
    pub fn new(
        source_node_id: impl Into<String>,
        content: Box<dyn std::any::Any + Send + Sync>,
        type_tag: TypeTag,
    ) -> Self {
        Self {
            message_id: uuid::Uuid::new_v4().to_string(),
            source_node_id: source_node_id.into(),
            target_node_id: None,
            content,
            type_tag,
            metadata: HashMap::new(),
            created_at: chrono::Utc::now(),
        }
    }

    pub fn with_target(mut self, target_id: impl Into<String>) -> Self {
        self.target_node_id = Some(target_id.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

impl Clone for MessageEnvelope {
    fn clone(&self) -> Self {
        Self {
            message_id: self.message_id.clone(),
            source_node_id: self.source_node_id.clone(),
            target_node_id: self.target_node_id.clone(),
            // content 不可 clone，用占位
            content: Box::new(()),
            type_tag: self.type_tag.clone(),
            metadata: self.metadata.clone(),
            created_at: self.created_at,
        }
    }
}

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_agent_core::{ChatMessage, Result};
use serde::{Deserialize, Serialize};

use crate::engine::MessageEnvelope;
use crate::executor::TypeTag;

/// 可序列化的 MessageEnvelope — 用于检查点持久化
///
/// 与 MessageEnvelope 的区别：
/// - `content` 字段替换为 `content_json: serde_json::Value`
/// - 通过 TypeTag 分发序列化/反序列化逻辑
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableMessageEnvelope {
    pub message_id: String,
    pub source_node_id: String,
    pub target_node_id: Option<String>,
    pub type_tag: TypeTag,
    pub content_json: serde_json::Value,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

// ═══════════════════════════════════════════════════
// MessageEnvelope → SerializableMessageEnvelope
// ═══════════════════════════════════════════════════

impl MessageEnvelope {
    /// 将 MessageEnvelope 转换为可序列化形态
    ///
    /// 根据 type_tag 分发序列化策略：
    /// - "ChatMessage" → 使用 serde 序列化
    /// - "Vec<ChatMessage>" → 使用 serde 序列化
    /// - 其他 → 尝试 serde_json::to_value，失败则用 Debug 字符串兜底
    pub fn to_serializable(&self) -> SerializableMessageEnvelope {
        let content_json = serialize_content(&self.type_tag, &self.content);

        SerializableMessageEnvelope {
            message_id: self.message_id.clone(),
            source_node_id: self.source_node_id.clone(),
            target_node_id: self.target_node_id.clone(),
            type_tag: self.type_tag.clone(),
            content_json,
            metadata: self.metadata.clone(),
            created_at: self.created_at,
        }
    }
}

/// 根据类型标签分发序列化
fn serialize_content(type_tag: &TypeTag, content: &Box<dyn std::any::Any + Send + Sync>) -> serde_json::Value {
    let type_name = type_tag.type_name.as_str();

    match type_name {
        // 核心类型：ChatMessage
        "ChatMessage" => {
            if let Some(msg) = content.downcast_ref::<ChatMessage>() {
                return serde_json::to_value(msg).unwrap_or(serde_json::Value::Null);
            }
        }
        // 核心类型：Vec<ChatMessage>
        "Vec<ChatMessage>" => {
            if let Some(msgs) = content.downcast_ref::<Vec<ChatMessage>>() {
                return serde_json::to_value(msgs).unwrap_or(serde_json::Value::Null);
            }
        }
        // 核心类型：String
        "String" => {
            if let Some(s) = content.downcast_ref::<String>() {
                return serde_json::Value::String(s.clone());
            }
        }
        _ => {}
    }

    // 兜底：尝试序列化为 JSON（如果内容实现了 Serialize）
    // 由于 `dyn Any` 擦除了类型，这里只能使用 Debug 格式兜底
    serde_json::Value::String(format!("{:?}", type_name))
}

// ═══════════════════════════════════════════════════
// SerializableMessageEnvelope → MessageEnvelope
// ═══════════════════════════════════════════════════

impl SerializableMessageEnvelope {
    /// 将可序列化消息信封还原为运行时 MessageEnvelope
    ///
    /// 根据 type_tag 分发反序列化：
    /// - "ChatMessage" → 反序列化为 ChatMessage
    /// - "Vec<ChatMessage>" → 反序列化为 Vec<ChatMessage>
    /// - "String" → 提取字符串
    /// - 其他 → 保留为 serde_json::Value（可在 Executor 中按需处理）
    pub fn into_message_envelope(self) -> Result<MessageEnvelope> {
        let content: Box<dyn std::any::Any + Send + Sync> =
            deserialize_content(&self.type_tag, &self.content_json)?;

        Ok(MessageEnvelope {
            message_id: self.message_id,
            source_node_id: self.source_node_id,
            target_node_id: self.target_node_id,
            content,
            type_tag: self.type_tag,
            metadata: self.metadata,
            created_at: self.created_at,
        })
    }
}

/// 根据类型标签分发反序列化
fn deserialize_content(
    type_tag: &TypeTag,
    json: &serde_json::Value,
) -> Result<Box<dyn std::any::Any + Send + Sync>> {
    let type_name = type_tag.type_name.as_str();

    match type_name {
        "ChatMessage" => {
            let msg: ChatMessage = serde_json::from_value(json.clone()).map_err(|e| {
                rust_agent_core::AgentError::Serialize(format!("反序列化 ChatMessage 失败: {}", e))
            })?;
            Ok(Box::new(msg))
        }
        "Vec<ChatMessage>" => {
            let msgs: Vec<ChatMessage> = serde_json::from_value(json.clone()).map_err(|e| {
                rust_agent_core::AgentError::Serialize(format!("反序列化 Vec<ChatMessage> 失败: {}", e))
            })?;
            Ok(Box::new(msgs))
        }
        "String" => {
            if let serde_json::Value::String(s) = json {
                Ok(Box::new(s.clone()))
            } else {
                Ok(Box::new(json.to_string()))
            }
        }
        _ => {
            // 未知类型：保留为 serde_json::Value，Executor 可自行处理
            Ok(Box::new(json.clone()))
        }
    }
}

// ═══════════════════════════════════════════════════
// 批量转换工具
// ═══════════════════════════════════════════════════

/// 将 Vec<MessageEnvelope> 批量转换为可序列化形态
pub fn serialize_envelopes(
    envelopes: &[MessageEnvelope],
) -> Vec<SerializableMessageEnvelope> {
    envelopes.iter().map(|e| e.to_serializable()).collect()
}

/// 将 Vec<SerializableMessageEnvelope> 批量还原为 MessageEnvelope
pub fn deserialize_envelopes(
    serializables: Vec<SerializableMessageEnvelope>,
) -> Result<Vec<MessageEnvelope>> {
    serializables
        .into_iter()
        .map(|s| s.into_message_envelope())
        .collect()
}

// ═══════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ChatMessage;

    #[test]
    fn test_roundtrip_chat_message() {
        let msg = ChatMessage::user("Hello, world!");
        let envelope = MessageEnvelope::new(
            "node_a",
            Box::new(msg.clone()),
            TypeTag::new("ChatMessage"),
        );

        let serializable = envelope.to_serializable();
        let restored = serializable.into_message_envelope().unwrap();

        let restored_msg = restored.content.downcast_ref::<ChatMessage>().unwrap();
        assert_eq!(restored_msg.content, "Hello, world!");
        assert_eq!(restored_msg.role, msg.role);
    }

    #[test]
    fn test_roundtrip_vec_chat_message() {
        let msgs = vec![
            ChatMessage::system("System prompt"),
            ChatMessage::user("User query"),
        ];
        let envelope = MessageEnvelope::new(
            "node_b",
            Box::new(msgs.clone()),
            TypeTag::new("Vec<ChatMessage>"),
        );

        let serializable = envelope.to_serializable();
        let restored = serializable.into_message_envelope().unwrap();

        let restored_msgs = restored.content.downcast_ref::<Vec<ChatMessage>>().unwrap();
        assert_eq!(restored_msgs.len(), 2);
        assert_eq!(restored_msgs[0].content, "System prompt");
    }

    #[test]
    fn test_roundtrip_string() {
        let envelope = MessageEnvelope::new(
            "node_c",
            Box::new("plain text".to_string()),
            TypeTag::new("String"),
        );

        let serializable = envelope.to_serializable();
        let restored = serializable.into_message_envelope().unwrap();

        let restored_str = restored.content.downcast_ref::<String>().unwrap();
        assert_eq!(restored_str, "plain text");
    }

    #[test]
    fn test_batch_conversion() {
        let envelopes: Vec<MessageEnvelope> = (0..3)
            .map(|i| {
                MessageEnvelope::new(
                    format!("node_{}", i),
                    Box::new(ChatMessage::user(format!("msg_{}", i))),
                    TypeTag::new("ChatMessage"),
                )
            })
            .collect();

        let serialized = serialize_envelopes(&envelopes);
        assert_eq!(serialized.len(), 3);

        let restored = deserialize_envelopes(serialized).unwrap();
        assert_eq!(restored.len(), 3);
        for (i, env) in restored.iter().enumerate() {
            let msg = env.content.downcast_ref::<ChatMessage>().unwrap();
            assert_eq!(msg.content, format!("msg_{}", i));
        }
    }
}

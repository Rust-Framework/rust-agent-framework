use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use rust_agent_core::{ChatMessage, Result};
use serde::{Deserialize, Serialize};

use crate::engine::MessageEnvelope;
use crate::executor::TypeTag;

/// 可序列化的 MessageEnvelope — 用于检查点持久化
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

impl MessageEnvelope {
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

fn serialize_content(
    type_tag: &TypeTag,
    content: &Arc<dyn std::any::Any + Send + Sync>,
) -> serde_json::Value {
    let type_name = type_tag.type_name.as_str();
    match type_name {
        "ChatMessage" => {
            if let Some(msg) = content.downcast_ref::<ChatMessage>() {
                return serde_json::to_value(msg).unwrap_or(serde_json::Value::Null);
            }
        }
        "Vec<ChatMessage>" => {
            if let Some(msgs) = content.downcast_ref::<Vec<ChatMessage>>() {
                return serde_json::to_value(msgs).unwrap_or(serde_json::Value::Null);
            }
        }
        "String" => {
            if let Some(s) = content.downcast_ref::<String>() {
                return serde_json::Value::String(s.clone());
            }
        }
        _ => {}
    }
    serde_json::Value::String(format!("{:?}", type_name))
}

impl SerializableMessageEnvelope {
    pub fn into_message_envelope(self) -> Result<MessageEnvelope> {
        let content: Arc<dyn std::any::Any + Send + Sync> =
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

fn deserialize_content(
    type_tag: &TypeTag,
    json: &serde_json::Value,
) -> Result<Arc<dyn std::any::Any + Send + Sync>> {
    let type_name = type_tag.type_name.as_str();
    match type_name {
        "ChatMessage" => {
            let msg: ChatMessage = serde_json::from_value(json.clone()).map_err(|e| {
                rust_agent_core::AgentError::Serialize(format!("反序列化 ChatMessage 失败: {}", e))
            })?;
            Ok(Arc::new(msg))
        }
        "Vec<ChatMessage>" => {
            let msgs: Vec<ChatMessage> = serde_json::from_value(json.clone()).map_err(|e| {
                rust_agent_core::AgentError::Serialize(format!(
                    "反序列化 Vec<ChatMessage> 失败: {}",
                    e
                ))
            })?;
            Ok(Arc::new(msgs))
        }
        "String" => {
            if let serde_json::Value::String(s) = json {
                Ok(Arc::new(s.clone()))
            } else {
                Ok(Arc::new(json.to_string()))
            }
        }
        _ => {
            Ok(Arc::new(json.clone()))
        }
    }
}

pub fn serialize_envelopes(
    envelopes: &[MessageEnvelope],
) -> Vec<SerializableMessageEnvelope> {
    envelopes.iter().map(|e| e.to_serializable()).collect()
}

pub fn deserialize_envelopes(
    serializables: Vec<SerializableMessageEnvelope>,
) -> Result<Vec<MessageEnvelope>> {
    serializables
        .into_iter()
        .map(|s| s.into_message_envelope())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ChatMessage;

    #[test]
    fn test_roundtrip_chat_message() {
        let msg = ChatMessage::user("Hello, world!");
        let envelope = MessageEnvelope::new(
            "node_a",
            Arc::new(msg.clone()),
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
            Arc::new(msgs.clone()),
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
            Arc::new("plain text".to_string()),
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
                    Arc::new(ChatMessage::user(format!("msg_{}", i))),
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

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rust_agent_core::Result;

use crate::engine::message_envelope::MessageEnvelope;

/// 消息关联键 —— 用于将入站消息与等待中的流程实例匹配。
#[derive(Debug, Clone)]
pub struct CorrelationKey {
    /// 业务主键
    pub business_key: Option<String>,
    /// 流程实例 ID
    pub process_id: Option<String>,
    /// 自定义键值对（全部匹配）
    pub custom_keys: HashMap<String, String>,
}

impl CorrelationKey {
    /// 仅通过业务主键关联。
    pub fn by_business_key(key: impl Into<String>) -> Self {
        Self {
            business_key: Some(key.into()),
            process_id: None,
            custom_keys: HashMap::new(),
        }
    }

    /// 通过流程实例 ID 关联。
    pub fn by_process_id(id: impl Into<String>) -> Self {
        Self {
            business_key: None,
            process_id: Some(id.into()),
            custom_keys: HashMap::new(),
        }
    }

    /// 添加自定义关联键。
    pub fn with_custom_key(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_keys.insert(key.into(), value.into());
        self
    }

    /// 判断消息信封是否匹配此关联键。
    ///
    /// 从 envelope.metadata 中查找关联字段（全部按 AND 逻辑匹配）。
    pub fn matches(&self, envelope: &MessageEnvelope) -> bool {
        let meta = &envelope.metadata;

        if let Some(ref bk) = self.business_key {
            match meta.get("business_key").and_then(|v| v.as_str()) {
                Some(v) if v == bk => {}
                _ => return false,
            }
        }

        if let Some(ref pid) = self.process_id {
            match meta.get("process_id").and_then(|v| v.as_str()) {
                Some(v) if v == pid => {}
                _ => return false,
            }
        }

        for (k, v) in &self.custom_keys {
            match meta.get(k).and_then(|mv| mv.as_str()) {
                Some(mv) if mv == v => {}
                _ => return false,
            }
        }

        true
    }
}

/// 消息关联器 —— 作为 IEdgeCondition 对入站消息进行关联匹配。
///
/// 附加在 DirectEdge 上：匹配成功 → 消息路由到目标节点，
/// 匹配失败 → 消息被丢弃（相当于等待）。
///
/// 可配合 timeout 使用：创建一个排他网关，一条边带此 condition，
/// 另一条边带 timeout condition。
#[derive(Clone)]
pub struct MessageCorrelation {
    key: CorrelationKey,
    timeout: Option<Duration>,
    started_at: Arc<parking_lot::Mutex<Option<Instant>>>,
}

impl std::fmt::Debug for MessageCorrelation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageCorrelation")
            .field("key", &self.key)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl MessageCorrelation {
    /// 使用指定关联键创建关联器。
    pub fn new(key: CorrelationKey) -> Self {
        Self {
            key,
            timeout: None,
            started_at: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn start(&self) {
        let mut guard = self.started_at.lock();
        if guard.is_none() {
            *guard = Some(Instant::now());
        }
    }

    pub fn reset(&self) {
        *self.started_at.lock() = None;
    }

    pub fn is_timed_out(&self) -> bool {
        let timeout = match self.timeout {
            Some(t) => t,
            None => return false,
        };
        let started = match *self.started_at.lock() {
            Some(s) => s,
            None => return false,
        };
        started.elapsed() >= timeout
    }
}

#[async_trait]
impl crate::graph::edge::IEdgeCondition for MessageCorrelation {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> Result<bool> {
        Ok(self.key.matches(envelope))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::message_envelope::MessageEnvelope;
    use crate::executor::TypeTag;

    fn make_envelope(kv: Vec<(&str, &str)>) -> MessageEnvelope {
        let msg: Arc<dyn std::any::Any + Send + Sync> = Arc::new("test".to_string());
        let mut env = MessageEnvelope::new("source", msg, TypeTag::new("test"));
        for (k, v) in kv {
            env = env.with_metadata(k, serde_json::Value::String(v.into()));
        }
        env
    }

    #[test]
    fn test_match_business_key() {
        let key = CorrelationKey::by_business_key("order-123");
        let env = make_envelope(vec![("business_key", "order-123")]);
        assert!(key.matches(&env));
    }

    #[test]
    fn test_mismatch_business_key() {
        let key = CorrelationKey::by_business_key("order-123");
        let env = make_envelope(vec![("business_key", "order-456")]);
        assert!(!key.matches(&env));
    }

    #[test]
    fn test_match_process_id() {
        let key = CorrelationKey::by_process_id("proc-1");
        let env = make_envelope(vec![("process_id", "proc-1")]);
        assert!(key.matches(&env));
    }

    #[test]
    fn test_match_custom_key() {
        let key = CorrelationKey {
            business_key: None,
            process_id: None,
            custom_keys: HashMap::from([("type".into(), "payment".into())]),
        };
        let env = make_envelope(vec![("type", "payment"), ("other", "ignored")]);
        assert!(key.matches(&env));
    }

    #[test]
    fn test_mismatch_custom_key() {
        let key = CorrelationKey {
            business_key: None,
            process_id: None,
            custom_keys: HashMap::from([("type".into(), "payment".into())]),
        };
        let env = make_envelope(vec![("type", "refund")]);
        assert!(!key.matches(&env));
    }

    #[test]
    fn test_match_multiple_keys() {
        let key = CorrelationKey {
            business_key: Some("bk-1".into()),
            process_id: Some("pid-1".into()),
            custom_keys: HashMap::from([("type".into(), "payment".into())]),
        };
        let env = make_envelope(vec![
            ("business_key", "bk-1"),
            ("process_id", "pid-1"),
            ("type", "payment"),
        ]);
        assert!(key.matches(&env));
    }

    #[test]
    fn test_mismatch_partial_keys() {
        let key = CorrelationKey {
            business_key: Some("bk-1".into()),
            process_id: Some("pid-1".into()),
            custom_keys: HashMap::from([("type".into(), "payment".into())]),
        };
        let env = make_envelope(vec![("business_key", "bk-1"), ("type", "payment")]);
        assert!(!key.matches(&env));
    }

    #[test]
    fn test_empty_key_matches_anything() {
        let key = CorrelationKey {
            business_key: None,
            process_id: None,
            custom_keys: HashMap::new(),
        };
        let env = make_envelope(vec![]);
        assert!(key.matches(&env));
    }

    #[test]
    fn test_timeout_initial_state() {
        let mc = MessageCorrelation::new(CorrelationKey::by_business_key("test"));
        assert!(!mc.is_timed_out());
    }

    #[test]
    fn test_timeout_after_start() {
        let mc = MessageCorrelation::new(CorrelationKey::by_business_key("test"))
            .with_timeout(Duration::from_millis(10));
        mc.start();
        std::thread::sleep(Duration::from_millis(20));
        assert!(mc.is_timed_out());
    }
}

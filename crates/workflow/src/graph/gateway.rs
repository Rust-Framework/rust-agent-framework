use async_trait::async_trait;
use rust_agent_core::Result;

use crate::engine::message_envelope::MessageEnvelope;
use crate::engine::boundary_event::BoundaryEventKind;

/// 事件驱动的排他网关条件 —— 对应 BPMN EventBasedGateway。
pub struct EventBasedGatewayCondition {
    pub expected_kind: BoundaryEventKind,
    pub event_id: Option<String>,
}

impl EventBasedGatewayCondition {
    pub fn new(kind: BoundaryEventKind) -> Self {
        Self {
            expected_kind: kind,
            event_id: None,
        }
    }

    pub fn with_event_id(mut self, id: impl Into<String>) -> Self {
        self.event_id = Some(id.into());
        self
    }
}

#[async_trait]
impl crate::graph::edge::IEdgeCondition for EventBasedGatewayCondition {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> Result<bool> {
        let meta = &envelope.metadata;
        let matched = match &self.expected_kind {
            BoundaryEventKind::Timer(_) => meta
                .get("boundary_timer_fired")
                .and_then(|v| v.as_str())
                .map(|v| self.event_id.as_ref().map_or(true, |eid| v == eid))
                .unwrap_or(false),
            BoundaryEventKind::Error(code) => {
                meta.get("error_code").and_then(|v| v.as_str()) == Some(code.as_str())
            }
            BoundaryEventKind::Signal(name) => {
                meta.get("signal_name").and_then(|v| v.as_str()) == Some(name.as_str())
            }
            BoundaryEventKind::Message(name) => {
                meta.get("message_name").and_then(|v| v.as_str()) == Some(name.as_str())
            }
            BoundaryEventKind::Escalation(code) => {
                meta.get("escalation_code").and_then(|v| v.as_str()) == Some(code.as_str())
            }
            BoundaryEventKind::Compensation => meta
                .get("compensation_triggered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        };
        Ok(matched)
    }
}

/// 复杂条件网关 —— 对应 BPMN ComplexGateway，支持 AND/OR 组合条件。
#[derive(Debug, Clone)]
pub struct ComplexGatewayCondition {
    pub combine: ConditionCombine,
    pub sub_conditions: Vec<SubCondition>,
}

#[derive(Debug, Clone)]
pub enum ConditionCombine {
    AllOf,
    AnyOf,
}

#[derive(Debug, Clone)]
pub struct SubCondition {
    pub variable: String,
    pub operator: ComparisonOperator,
    pub expected: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum ComparisonOperator {
    Equals,
    NotEquals,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Contains,
    StartsWith,
}

impl SubCondition {
    pub fn evaluate(&self, actual: &serde_json::Value) -> bool {
        match self.operator {
            ComparisonOperator::Equals => actual == &self.expected,
            ComparisonOperator::NotEquals => actual != &self.expected,
            ComparisonOperator::Contains => {
                let a = actual.as_str().unwrap_or_default();
                let e = self.expected.as_str().unwrap_or_default();
                a.contains(e)
            }
            ComparisonOperator::StartsWith => {
                let a = actual.as_str().unwrap_or_default();
                let e = self.expected.as_str().unwrap_or_default();
                a.starts_with(e)
            }
            ComparisonOperator::GreaterThan
            | ComparisonOperator::GreaterThanOrEqual
            | ComparisonOperator::LessThan
            | ComparisonOperator::LessThanOrEqual => {
                let a = actual.as_f64();
                let e = self.expected.as_f64();
                match (a, e) {
                    (Some(a), Some(e)) => match self.operator {
                        ComparisonOperator::GreaterThan => a > e,
                        ComparisonOperator::GreaterThanOrEqual => a >= e,
                        ComparisonOperator::LessThan => a < e,
                        ComparisonOperator::LessThanOrEqual => a <= e,
                        _ => false,
                    },
                    _ => false,
                }
            }
        }
    }
}

impl ComplexGatewayCondition {
    pub fn all_of(sub_conditions: Vec<SubCondition>) -> Self {
        Self {
            combine: ConditionCombine::AllOf,
            sub_conditions,
        }
    }

    pub fn any_of(sub_conditions: Vec<SubCondition>) -> Self {
        Self {
            combine: ConditionCombine::AnyOf,
            sub_conditions,
        }
    }
}

#[async_trait]
impl crate::graph::edge::IEdgeCondition for ComplexGatewayCondition {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> Result<bool> {
        let meta = &envelope.metadata;
        let results: Vec<bool> = self
            .sub_conditions
            .iter()
            .map(|cond| {
                let value = meta
                    .get(&cond.variable)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                cond.evaluate(&value)
            })
            .collect();
        Ok(match self.combine {
            ConditionCombine::AllOf => results.iter().all(|&r| r),
            ConditionCombine::AnyOf => results.iter().any(|&r| r),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::graph::edge::IEdgeCondition;
    use crate::engine::message_envelope::MessageEnvelope;
    use crate::executor::TypeTag;

    fn make_env(kv: Vec<(&str, serde_json::Value)>) -> MessageEnvelope {
        let msg: Arc<dyn std::any::Any + Send + Sync> = Arc::new("test".to_string());
        let mut env = MessageEnvelope::new("src", msg, TypeTag::new("test"));
        for (k, v) in kv {
            env = env.with_metadata(k, v);
        }
        env
    }

    #[test]
    fn test_complex_all_of_ok() {
        let c = ComplexGatewayCondition::all_of(vec![
            SubCondition { variable: "amount".into(), operator: ComparisonOperator::GreaterThan, expected: serde_json::json!(100) },
            SubCondition { variable: "approved".into(), operator: ComparisonOperator::Equals, expected: serde_json::json!(true) },
        ]);
        let env = make_env(vec![("amount", serde_json::json!(200)), ("approved", serde_json::json!(true))]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(rt.block_on(async { c.evaluate(&env).await.unwrap() }));
    }

    #[test]
    fn test_complex_all_of_fail() {
        let c = ComplexGatewayCondition::all_of(vec![
            SubCondition { variable: "amount".into(), operator: ComparisonOperator::GreaterThan, expected: serde_json::json!(100) },
            SubCondition { variable: "approved".into(), operator: ComparisonOperator::Equals, expected: serde_json::json!(true) },
        ]);
        let env = make_env(vec![("amount", serde_json::json!(50)), ("approved", serde_json::json!(true))]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(!rt.block_on(async { c.evaluate(&env).await.unwrap() }));
    }

    #[test]
    fn test_complex_any_of_ok() {
        let c = ComplexGatewayCondition::any_of(vec![
            SubCondition { variable: "amount".into(), operator: ComparisonOperator::GreaterThan, expected: serde_json::json!(100) },
            SubCondition { variable: "approved".into(), operator: ComparisonOperator::Equals, expected: serde_json::json!(true) },
        ]);
        let env = make_env(vec![("amount", serde_json::json!(50)), ("approved", serde_json::json!(true))]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(rt.block_on(async { c.evaluate(&env).await.unwrap() }));
    }

    #[test]
    fn test_event_gateway_error_match() {
        let c = EventBasedGatewayCondition::new(BoundaryEventKind::Error("TIMEOUT".into()));
        let env = make_env(vec![("error_code", serde_json::json!("TIMEOUT"))]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(rt.block_on(async { c.evaluate(&env).await.unwrap() }));
    }

    #[test]
    fn test_event_gateway_no_match() {
        let c = EventBasedGatewayCondition::new(BoundaryEventKind::Signal("CANCEL".into()));
        let env = make_env(vec![("signal_name", serde_json::json!("PAUSE"))]);
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(!rt.block_on(async { c.evaluate(&env).await.unwrap() }));
    }
}

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;

use crate::engine::message_envelope::MessageEnvelope;

/// 条件运算符
#[derive(Debug, Clone)]
pub enum ComparisonOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    StartsWith,
}

/// 基于流程变量的条件 — 从 state_map 读取变量进行比较
#[derive(Debug, Clone)]
pub struct VariableCondition {
    pub variable: String,
    pub operator: ComparisonOp,
    pub expected: serde_json::Value,
}

impl VariableCondition {
    pub fn new(variable: impl Into<String>, operator: ComparisonOp, expected: serde_json::Value) -> Self {
        Self {
            variable: variable.into(),
            operator,
            expected,
        }
    }

    fn evaluate_value(&self, actual: &serde_json::Value) -> bool {
        match self.operator {
            ComparisonOp::Eq => actual == &self.expected,
            ComparisonOp::Neq => actual != &self.expected,
            ComparisonOp::Contains => {
                let a = actual.as_str().unwrap_or_default();
                let e = self.expected.as_str().unwrap_or_default();
                a.contains(e)
            }
            ComparisonOp::StartsWith => {
                let a = actual.as_str().unwrap_or_default();
                let e = self.expected.as_str().unwrap_or_default();
                a.starts_with(e)
            }
            ComparisonOp::Gt | ComparisonOp::Gte | ComparisonOp::Lt | ComparisonOp::Lte => {
                let a = actual.as_f64();
                let e = self.expected.as_f64();
                match (a, e) {
                    (Some(a), Some(e)) => match self.operator {
                        ComparisonOp::Gt => a > e,
                        ComparisonOp::Gte => a >= e,
                        ComparisonOp::Lt => a < e,
                        ComparisonOp::Lte => a <= e,
                        _ => false,
                    },
                    _ => false,
                }
            }
        }
    }
}

/// 基于流程变量的 IEdgeCondition 实现
pub struct VariableEdgeCondition {
    pub condition: VariableCondition,
    /// 用于读取变量的 state_map（由引擎在 evaluate 前设置）
    pub state_reader: Option<Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>>,
}

impl VariableEdgeCondition {
    pub fn new(variable: impl Into<String>, operator: ComparisonOp, expected: serde_json::Value) -> Self {
        Self {
            condition: VariableCondition::new(variable, operator, expected),
            state_reader: None,
        }
    }
}

#[async_trait]
impl crate::graph::edge::IEdgeCondition for VariableEdgeCondition {
    async fn evaluate(&self, _envelope: &MessageEnvelope) -> Result<bool> {
        if let Some(ref reader) = self.state_reader {
            let state = reader.lock().await;
            let value = state.get(&self.condition.variable).cloned().unwrap_or(serde_json::Value::Null);
            Ok(self.condition.evaluate_value(&value))
        } else {
            Ok(false)
        }
    }
}

/// 表达式条件 — 多条件组合（AllOf / AnyOf）
pub struct ExpressionCondition {
    pub conditions: Vec<VariableCondition>,
    pub combine: ConditionCombine,
    /// 用于读取流程变量的 state_map
    pub state_reader: Option<Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>>,
}

#[derive(Debug, Clone)]
pub enum ConditionCombine {
    AllOf,
    AnyOf,
}

impl ExpressionCondition {
    pub fn all_of(conditions: Vec<VariableCondition>) -> Self {
        Self {
            conditions,
            combine: ConditionCombine::AllOf,
            state_reader: None,
        }
    }

    pub fn any_of(conditions: Vec<VariableCondition>) -> Self {
        Self {
            conditions,
            combine: ConditionCombine::AnyOf,
            state_reader: None,
        }
    }

    pub fn with_state_reader(
        mut self,
        reader: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    ) -> Self {
        self.state_reader = Some(reader);
        self
    }
}

#[async_trait]
impl crate::graph::edge::IEdgeCondition for ExpressionCondition {
    async fn evaluate(&self, _envelope: &MessageEnvelope) -> Result<bool> {
        if self.conditions.is_empty() {
            return Ok(true);
        }

        let Some(ref reader) = self.state_reader else {
            return Ok(false);
        };

        let state = reader.lock().await;
        let results: Vec<bool> = self
            .conditions
            .iter()
            .map(|cond| {
                let value = state
                    .get(&cond.variable)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                cond.evaluate_value(&value)
            })
            .collect();

        Ok(match self.combine {
            ConditionCombine::AllOf => results.iter().all(|&r| r),
            ConditionCombine::AnyOf => results.iter().any(|&r| r),
        })
    }
}

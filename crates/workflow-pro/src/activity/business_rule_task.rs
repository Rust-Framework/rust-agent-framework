use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;
use rust_agent_workflow::executor::{IExecutor, HandlerResult, NodeProgress, TypeTag};
use rust_agent_workflow::engine::IWorkflowContext;

/// A single business rule definition.
#[derive(Debug, Clone)]
pub struct RuleDef {
    /// Human-readable name for diagnostics.
    pub name: String,
    /// A boolean expression evaluated against ctx state variables.
    /// Expressions use variable names from the workflow context.
    pub expression: String,
    /// The branch name to route the result to when this rule matches.
    pub result_branch: String,
}

impl RuleDef {
    pub fn new(
        name: impl Into<String>,
        expression: impl Into<String>,
        result_branch: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            expression: expression.into(),
            result_branch: result_branch.into(),
        }
    }
}

/// Evaluates a set of business rules against workflow context state
/// and routes the result to the appropriate branch.
///
/// Rules are evaluated in order; the first matching rule determines
/// the outcome. If no rule matches, the message passes through unmodified.
pub struct BusinessRuleTask {
    pub node_id: String,
    pub rules: Vec<RuleDef>,
    /// Engine type: "expression" (default) for simple boolean evaluation.
    pub engine_type: String,
}

impl BusinessRuleTask {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            rules: Vec::new(),
            engine_type: "expression".to_string(),
        }
    }

    pub fn with_rules(mut self, rules: Vec<RuleDef>) -> Self {
        self.rules = rules;
        self
    }

    pub fn with_engine_type(mut self, engine_type: impl Into<String>) -> Self {
        self.engine_type = engine_type.into();
        self
    }
}

#[async_trait]
impl IExecutor for BusinessRuleTask {
    fn id(&self) -> &str {
        &self.node_id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("initial")]
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        tracing::info!(
            node_id = %self.node_id,
            engine_type = %self.engine_type,
            rules_count = %self.rules.len(),
            "BusinessRuleTask evaluating rules"
        );

        // Evaluate rules in order against context state variables.
        // Each rule's expression references variable names stored in the
        // workflow context. The first matching rule determines routing.
        for rule in &self.rules {
            let matched = self.evaluate_expression(&rule.expression, &ctx).await;

            let _ = progress.send(NodeProgress::Custom {
                key: "business_rule.evaluated".into(),
                value: serde_json::json!({
                    "rule_name": rule.name,
                    "expression": rule.expression,
                    "matched": matched,
                }),
            });

            if matched {
                tracing::info!(
                    node_id = %self.node_id,
                    rule_name = %rule.name,
                    result_branch = %rule.result_branch,
                    "BusinessRuleTask matched rule, routing to branch"
                );

                // Attach the matched branch name as metadata on the message
                // for downstream gateway routing.
                let branch_msg: Arc<dyn std::any::Any + Send + Sync> = Arc::new(
                    serde_json::json!({
                        "branch": &rule.result_branch,
                        "matched_rule": &rule.name,
                    })
                );

                return Ok(HandlerResult::Messages(vec![branch_msg]));
            }
        }

        // No rule matched — pass the original message through
        tracing::info!(
            node_id = %self.node_id,
            "BusinessRuleTask no rule matched, passing through"
        );

        Ok(HandlerResult::Messages(vec![message]))
    }
}

impl BusinessRuleTask {
    /// Evaluates a single expression against workflow context state.
    ///
    /// Placeholder: replaces variable references in the expression with
    /// their values from context state, then evaluates as a simple
    /// equality or truthiness check.
    async fn evaluate_expression(
        &self,
        expression: &str,
        ctx: &Arc<dyn IWorkflowContext>,
    ) -> bool {
        // Placeholder implementation: look up each token as a variable
        // and check if it is truthy. In production this would use an
        // expression engine (e.g. `evalexpr` or `rhai`).
        let trimmed = expression.trim();

        // Handle simple boolean literals
        if trimmed.eq_ignore_ascii_case("true") {
            return true;
        }
        if trimmed.eq_ignore_ascii_case("false") {
            return false;
        }

        // Handle "key == value" pattern
        if let Some((lhs, rhs)) = trimmed.split_once("==") {
            let lhs = lhs.trim();
            let rhs = rhs.trim().trim_matches('"').trim_matches('\'');

            match ctx.get_variable(lhs).await {
                Ok(Some(val)) => {
                    let matches = match val {
                        serde_json::Value::String(s) => s == rhs,
                        serde_json::Value::Bool(b) => {
                            b == rhs.eq_ignore_ascii_case("true")
                        }
                        other => other.to_string() == rhs,
                    };
                    return matches;
                }
                _ => return false,
            }
        }

        // Handle "key != value" pattern
        if let Some((lhs, rhs)) = trimmed.split_once("!=") {
            let lhs = lhs.trim();
            let rhs = rhs.trim().trim_matches('"').trim_matches('\'');

            match ctx.get_variable(lhs).await {
                Ok(Some(val)) => {
                    let matches = match val {
                        serde_json::Value::String(s) => s != rhs,
                        serde_json::Value::Bool(b) => {
                            b != rhs.eq_ignore_ascii_case("true")
                        }
                        other => other.to_string() != rhs,
                    };
                    return matches;
                }
                _ => return true, // Variable not set — "!=" evaluates true
            }
        }

        // Treat the expression as a variable name and check its truthiness
        match ctx.get_variable(trimmed).await {
            Ok(Some(val)) => {
                match val {
                    serde_json::Value::Bool(b) => b,
                    serde_json::Value::Null => false,
                    serde_json::Value::String(s) => !s.is_empty(),
                    serde_json::Value::Number(_) => true,
                    serde_json::Value::Array(a) => !a.is_empty(),
                    serde_json::Value::Object(_) => true,
                }
            }
            _ => false,
        }
    }
}

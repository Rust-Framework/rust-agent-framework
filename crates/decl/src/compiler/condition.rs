//! 工作流条件求值 — RAF 轻量解析 + 可选 Rhai 统一表达式。

#[cfg(feature = "rhai")]
use std::collections::HashMap;

use rust_agent_workflow::engine::IWorkflowContext;

/// 从 workflow state 求值布尔条件。
pub async fn evaluate_workflow_condition(expr: &str, ctx: &dyn IWorkflowContext) -> bool {
    let expr = expr.trim().strip_prefix('=').unwrap_or(expr).trim();

    if is_pure_state_reference(expr) {
        if let Some(resolved) = resolve_state_reference(expr, ctx).await {
            return json_to_bool(&resolved);
        }
    }

    #[cfg(feature = "rhai")]
    {
        if let Ok(result) = evaluate_rhai_condition(expr, ctx).await {
            return result;
        }
    }

    evaluate_simple_condition(expr, ctx).await
}

#[cfg(feature = "rhai")]
async fn evaluate_rhai_condition(
    expr: &str,
    ctx: &dyn IWorkflowContext,
) -> Result<bool, ()> {
    let mut state = HashMap::new();

    for key in rust_agent_rhai::extract_all_state_keys(expr) {
        if let Ok(Some(val)) = ctx.read_state(&key).await {
            state.insert(key, val);
        }
    }

    for key in [
        "response",
        "result",
        "__last_activity",
        "__invoke_response",
        "__external_loop_continue",
    ] {
        if state.contains_key(key) {
            continue;
        }
        if let Ok(Some(val)) = ctx.read_state(key).await {
            state.insert(key.to_string(), val);
        }
    }

    rust_agent_rhai::eval_workflow_bool(expr, &state).map_err(|_| ())
}

fn is_pure_state_reference(expr: &str) -> bool {
    let token = expr
        .strip_prefix("Local.")
        .or_else(|| expr.strip_prefix("System."))
        .unwrap_or(expr);
    !token.contains([' ', '=', '<', '>', '!'])
}

async fn resolve_state_reference(
    expr: &str,
    ctx: &dyn IWorkflowContext,
) -> Option<serde_json::Value> {
    let key = expr
        .strip_prefix("Local.")
        .or_else(|| expr.strip_prefix("System."))
        .unwrap_or(expr);
    let storage_key = if expr.starts_with("System.") {
        format!("sys_{key}")
    } else {
        key.to_string()
    };
    ctx.read_state(&storage_key).await.ok().flatten()
}

fn json_to_bool(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        serde_json::Value::String(s) => {
            matches!(s.to_lowercase().as_str(), "true" | "yes" | "1")
        }
        serde_json::Value::Null => false,
        _ => true,
    }
}

async fn evaluate_simple_condition(expr: &str, ctx: &dyn IWorkflowContext) -> bool {
    if let Some((left, right)) = expr.split_once(" contains ") {
        let l = resolve_token(left.trim(), ctx).await;
        let r = resolve_token(right.trim(), ctx).await;
        return l.contains(&r);
    }
    if let Some((a, b)) = expr.split_once(" == ") {
        return resolve_token(a.trim(), ctx).await == resolve_token(b.trim(), ctx).await;
    }
    if let Some((a, b)) = expr.split_once(" != ") {
        return resolve_token(a.trim(), ctx).await != resolve_token(b.trim(), ctx).await;
    }
    if let Some((a, b)) = expr.split_once(" >= ") {
        if let (Ok(a), Ok(b)) = (
            resolve_token(a.trim(), ctx).await.parse::<f64>(),
            resolve_token(b.trim(), ctx).await.parse::<f64>(),
        ) {
            return a >= b;
        }
    }
    if let Some((a, b)) = expr.split_once(" <= ") {
        if let (Ok(a), Ok(b)) = (
            resolve_token(a.trim(), ctx).await.parse::<f64>(),
            resolve_token(b.trim(), ctx).await.parse::<f64>(),
        ) {
            return a <= b;
        }
    }
    if let Some((a, b)) = expr.split_once(" > ") {
        if let (Ok(a), Ok(b)) = (
            resolve_token(a.trim(), ctx).await.parse::<f64>(),
            resolve_token(b.trim(), ctx).await.parse::<f64>(),
        ) {
            return a > b;
        }
        return false;
    }
    if let Some((a, b)) = expr.split_once(" < ") {
        if let (Ok(a), Ok(b)) = (
            resolve_token(a.trim(), ctx).await.parse::<f64>(),
            resolve_token(b.trim(), ctx).await.parse::<f64>(),
        ) {
            return a < b;
        }
        return false;
    }

    let substituted = resolve_token(expr, ctx).await;
    match substituted.to_lowercase().as_str() {
        "true" | "yes" | "1" => true,
        "false" | "no" | "0" | "" => false,
        _ => false,
    }
}

async fn resolve_token(token: &str, ctx: &dyn IWorkflowContext) -> String {
    let (prefix, key) = if let Some(k) = token.strip_prefix("Local.") {
        ("Local.", k)
    } else if let Some(k) = token.strip_prefix("System.") {
        ("System.", k)
    } else {
        return token.to_string();
    };
    let storage_key = if prefix == "System." {
        format!("sys_{key}")
    } else {
        key.to_string()
    };
    if let Ok(Some(val)) = ctx.read_state(&storage_key).await {
        return value_to_string(&val);
    }
    token.to_string()
}

fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use rust_agent_workflow::engine::{IWorkflowContext, MessageEnvelope, WorkflowEvent};
    use tokio::sync::RwLock;

    use super::*;

    struct MockCtx {
        state: RwLock<HashMap<String, serde_json::Value>>,
    }

    #[async_trait]
    impl IWorkflowContext for MockCtx {
        async fn send_message(&self, _envelope: MessageEnvelope) -> rust_agent_core::Result<()> {
            Ok(())
        }

        async fn yield_output(
            &self,
            _output: Arc<dyn std::any::Any + Send + Sync>,
        ) -> rust_agent_core::Result<()> {
            Ok(())
        }

        async fn emit_event(&self, _event: WorkflowEvent) {}

        async fn request_halt(&self) {}

        async fn read_state(
            &self,
            key: &str,
        ) -> rust_agent_core::Result<Option<serde_json::Value>> {
            Ok(self.state.read().await.get(key).cloned())
        }

        async fn write_state(
            &self,
            key: &str,
            value: serde_json::Value,
        ) -> rust_agent_core::Result<()> {
            self.state.write().await.insert(key.to_string(), value);
            Ok(())
        }

        async fn clear_state(&self, key: &str) -> rust_agent_core::Result<()> {
            self.state.write().await.remove(key);
            Ok(())
        }

        fn current_node_id(&self) -> &str {
            "test"
        }

        fn session(&self) -> Option<&Arc<dyn rust_agent_core::ISession>> {
            None
        }
    }

    #[tokio::test]
    async fn evaluates_state_reference() {
        let ctx = MockCtx {
            state: RwLock::new(HashMap::new()),
        };
        ctx.write_state("done", serde_json::json!(true)).await.unwrap();
        assert!(evaluate_workflow_condition("Local.done", &ctx).await);
    }

    #[tokio::test]
    async fn evaluates_equality_with_state() {
        let ctx = MockCtx {
            state: RwLock::new(HashMap::new()),
        };
        ctx.write_state("status", serde_json::json!("pending"))
            .await
            .unwrap();
        assert!(evaluate_workflow_condition("Local.status == pending", &ctx).await);
        assert!(!evaluate_workflow_condition("Local.status == done", &ctx).await);
    }

    #[tokio::test]
    async fn evaluates_numeric_comparison() {
        let ctx = MockCtx {
            state: RwLock::new(HashMap::new()),
        };
        ctx.write_state("count", serde_json::json!(3)).await.unwrap();
        assert!(evaluate_workflow_condition("Local.count < 5", &ctx).await);
        assert!(!evaluate_workflow_condition("Local.count > 10", &ctx).await);
    }

    #[cfg(feature = "rhai")]
    #[tokio::test]
    async fn evaluates_rhai_expression() {
        let ctx = MockCtx {
            state: RwLock::new(HashMap::new()),
        };
        ctx.write_state("count", serde_json::json!(4)).await.unwrap();
        assert!(evaluate_workflow_condition("Local.count >= 4 && Local.count <= 10", &ctx).await);
    }

    #[cfg(feature = "rhai")]
    #[tokio::test]
    async fn evaluates_rhai_dynamic_state_fn() {
        let ctx = MockCtx {
            state: RwLock::new(HashMap::new()),
        };
        ctx.write_state("ready", serde_json::json!(false))
            .await
            .unwrap();
        assert!(!evaluate_workflow_condition(r#"local("ready")"#, &ctx).await);
        ctx.write_state("ready", serde_json::json!(true)).await.unwrap();
        assert!(evaluate_workflow_condition(r#"local("ready")"#, &ctx).await);
    }
}

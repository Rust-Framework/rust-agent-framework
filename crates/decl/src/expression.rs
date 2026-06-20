//! RAF 表达式引擎 — 声明式字段的动态求值。
//!
//! - 环境变量：`$VAR_NAME`、`=Env.VAR_NAME`（YAML 兼容前缀，语义为 RAF env）
//! - Rhai 脚本：`=expr`（需 `rhai` feature），RAF 自有嵌入语言体系
//!
//! 工作流条件分支见 [`crate::compiler::condition`]（轻量比较 + 可选 Rhai 扩展）。

#[cfg(feature = "rhai")]
use std::collections::HashMap;

/// 表达式求值引擎。
pub struct ExpressionEngine {}

impl ExpressionEngine {
    pub fn new() -> Self {
        Self {}
    }

    /// 检查字符串值是否以 `=` 开头，表示 Rhai 表达式。
    pub fn is_expression(value: &str) -> bool {
        value.starts_with('=') && !value.starts_with("=Env.")
    }

    /// 解析环境变量引用（`$VAR` 或 `=Env.VAR`）。
    pub fn resolve_env(value: &str) -> Option<String> {
        if let Some(var_name) = value.strip_prefix('$') {
            std::env::var(var_name).ok()
        } else if let Some(var_name) = value.strip_prefix("=Env.") {
            std::env::var(var_name).ok()
        } else {
            None
        }
    }

    /// 解析环境变量或 Rhai 表达式；字面量原样返回。
    pub fn resolve_value(&self, value: &str) -> String {
        if let Some(resolved) = Self::resolve_env(value) {
            return resolved;
        }

        if Self::is_expression(value) {
            #[cfg(feature = "rhai")]
            {
                let expr = &value[1..];
                if let Ok(result) = self.evaluate_rhai(expr, None) {
                    return result;
                }
            }
        }

        value.to_string()
    }

    /// 求值 Rhai 表达式，返回字符串化结果。
    #[cfg(feature = "rhai")]
    pub fn evaluate_rhai(
        &self,
        expression: &str,
        variables: Option<&HashMap<String, serde_json::Value>>,
    ) -> Result<String, String> {
        let mut runtime = rust_agent_rhai::RhaiRuntime::new();
        if let Some(vars) = variables {
            for (k, v) in vars {
                runtime.with_json_variable(k, v.clone());
            }
        }
        let value = runtime
            .eval(expression)
            .map_err(|e| format!("Rhai evaluation error: {e}"))?;
        Ok(json_value_to_string(&value))
    }
}

impl Default for ExpressionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "rhai")]
fn json_value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_expression() {
        assert!(ExpressionEngine::is_expression("=1 + 2"));
        assert!(!ExpressionEngine::is_expression("=Env.API_KEY"));
        assert!(!ExpressionEngine::is_expression("Hello"));
    }

    #[test]
    fn test_resolve_env_dollar() {
        std::env::set_var("TEST_VAR_123", "test_value");
        let result = ExpressionEngine::resolve_env("$TEST_VAR_123");
        assert_eq!(result, Some("test_value".to_string()));
        std::env::remove_var("TEST_VAR_123");
    }

    #[test]
    fn test_resolve_env_prefix() {
        std::env::set_var("TEST_VAR_456", "test_value_2");
        let result = ExpressionEngine::resolve_env("=Env.TEST_VAR_456");
        assert_eq!(result, Some("test_value_2".to_string()));
        std::env::remove_var("TEST_VAR_456");
    }

    #[test]
    fn test_resolve_value_literal() {
        let engine = ExpressionEngine::new();
        assert_eq!(engine.resolve_value("Hello World"), "Hello World");
    }

    #[cfg(feature = "rhai")]
    #[test]
    fn test_rhai_arithmetic() {
        let engine = ExpressionEngine::new();
        assert_eq!(engine.resolve_value("=1 + 2"), "3");
    }
}

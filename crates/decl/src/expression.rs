//! Expression engine for declarative agent resolution.
//!
//! Provides:
//! - Environment variable resolution (`$VAR`, `=Env.VAR`)
//! - PowerFx expression evaluation (optional, feature-gated)
//! - Expression detection (`value.starts_with('=')`)
//!
//! ## Expression support scope (aligned with MAF)
//!
//! PowerFx expressions are supported on these fields:
//! - `AgentDefinition`: `name`, `displayName`, `description`
//! - `PromptAgent`: `instructions`, `additionalInstructions`
//! - `Model`: `id`, `provider`, `apiType`
//! - `Connection`: `endpoint`, `apiKey`, `name`, `target`
//! - `Tool`: `name`, `kind`, `description`
//!
//! NOT supported: `ModelOptions` (temperature, seed, etc.) — numeric fields.

#[cfg(feature = "powerfx")]
use powerfx::{DataValue, PowerFxEngine, Session};

/// 表达式求值引擎。
///
/// 支持环境变量解析和可选的 PowerFx 表达式求值（在 `powerfx` 特性标志后）。
pub struct ExpressionEngine {
    #[cfg(feature = "powerfx")]
    inner: PowerFxEngine,
}

impl ExpressionEngine {
    /// 创建新的表达式引擎。
    #[cfg(feature = "powerfx")]
    pub fn new() -> Self {
        Self {
            inner: PowerFxEngine::new(),
        }
    }

    /// 创建新的表达式引擎（无 PowerFx）。
    #[cfg(not(feature = "powerfx"))]
    pub fn new() -> Self {
        Self {}
    }

    /// 检查字符串值是否以 `=` 开头，表示一个表达式。
    pub fn is_expression(value: &str) -> bool {
        value.starts_with('=')
    }

    /// 解析环境变量引用。
    ///
    /// 支持两种格式：
    /// - `$VAR_NAME` — Unix 风格的环境变量引用
    /// - `=Env.VAR_NAME` — MAF PowerFx 环境变量引用
    ///
    /// 如果值看起来不像环境变量引用则返回 `None`。
    pub fn resolve_env(value: &str) -> Option<String> {
        if let Some(var_name) = value.strip_prefix('$') {
            std::env::var(var_name).ok()
        } else if let Some(var_name) = value.strip_prefix("=Env.") {
            std::env::var(var_name).ok()
        } else {
            None
        }
    }

    /// 解析可能是环境变量引用或表达式的值，无需解析时返回原值。
    pub fn resolve_value(&self, value: &str) -> String {
        // Try env var first
        if let Some(resolved) = Self::resolve_env(value) {
            return resolved;
        }

        // Try PowerFx expression
        if Self::is_expression(value) {
            #[cfg(feature = "powerfx")]
            {
                let expr = &value[1..]; // strip leading '='
                if let Ok(result) = self.evaluate_powerfx(expr, None) {
                    return result;
                }
            }
        }

        value.to_string()
    }

    /// 求值 PowerFx 表达式。
    #[cfg(feature = "powerfx")]
    pub fn evaluate_powerfx(
        &self,
        expression: &str,
        session: Option<&mut Session>,
    ) -> Result<String, String> {
        let result = self
            .inner
            .evaluate(expression, session)
            .map_err(|e| format!("PowerFx evaluation error: {}", e))?;

        match result {
            DataValue::Number(n) => Ok(n.to_string()),
            DataValue::String(s) => Ok(s),
            DataValue::Boolean(b) => Ok(b.to_string()),
            DataValue::DateTime(dt) => Ok(dt.to_string()),
            DataValue::Table(_) => Ok("[table]".to_string()),
            DataValue::Blank => Ok(String::new()),
            other => Ok(format!("{:?}", other)),
        }
    }

    /// 创建预填充变量的会话。
    #[cfg(feature = "powerfx")]
    pub fn create_session(variables: &std::collections::HashMap<String, serde_json::Value>) -> Session {
        let mut session = Session::new();
        for (key, value) in variables {
            let data_value = json_to_data_value(value);
            session.set_variable(key, data_value);
        }
        session
    }
}

impl Default for ExpressionEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// 将 `serde_json::Value` 转换为 `powerfx::DataValue`。
#[cfg(feature = "powerfx")]
fn json_to_data_value(value: &serde_json::Value) -> DataValue {
    match value {
        serde_json::Value::Null => DataValue::Blank,
        serde_json::Value::Bool(b) => DataValue::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                DataValue::Number(f)
            } else {
                DataValue::Blank
            }
        }
        serde_json::Value::String(s) => DataValue::String(s.clone()),
        serde_json::Value::Array(_) => DataValue::Blank,
        serde_json::Value::Object(_) => DataValue::Blank,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_expression() {
        assert!(ExpressionEngine::is_expression("=Hello"));
        assert!(!ExpressionEngine::is_expression("Hello"));
        assert!(!ExpressionEngine::is_expression(""));
    }

    #[test]
    fn test_resolve_env_dollar() {
        std::env::set_var("TEST_VAR_123", "test_value");
        let result = ExpressionEngine::resolve_env("$TEST_VAR_123");
        assert_eq!(result, Some("test_value".to_string()));
        std::env::remove_var("TEST_VAR_123");
    }

    #[test]
    fn test_resolve_env_powerfx() {
        std::env::set_var("TEST_VAR_456", "test_value_2");
        let result = ExpressionEngine::resolve_env("=Env.TEST_VAR_456");
        assert_eq!(result, Some("test_value_2".to_string()));
        std::env::remove_var("TEST_VAR_456");
    }

    #[test]
    fn test_resolve_env_missing() {
        let result = ExpressionEngine::resolve_env("$NONEXISTENT_VAR_XYZ");
        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_value_literal() {
        let engine = ExpressionEngine::new();
        let result = engine.resolve_value("Hello World");
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_resolve_value_env() {
        std::env::set_var("MY_KEY", "secret123");
        let engine = ExpressionEngine::new();
        let result = engine.resolve_value("$MY_KEY");
        assert_eq!(result, "secret123");
        std::env::remove_var("MY_KEY");
    }
}

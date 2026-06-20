//! RAF 工作流/声明式表达式求值辅助。

use std::collections::HashMap;

use rhai::Dynamic;
use rust_agent_core::Result;

use crate::runtime::{dynamic_to_json_val, RhaiRuntime};

/// 将 `Local.key` / `System.key` 转为 Rhai 变量引用（state 中已注入同名变量）。
pub fn normalize_workflow_expr(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(rest) = expr[i..].strip_prefix("Local.") {
            if let Some((key, consumed)) = parse_identifier(rest) {
                out.push_str(&key);
                i += "Local.".len() + consumed;
                continue;
            }
        }
        if let Some(rest) = expr[i..].strip_prefix("System.") {
            if let Some((key, consumed)) = parse_identifier(rest) {
                let sys_key = format!("sys_{key}");
                out.push_str(&sys_key);
                i += "System.".len() + consumed;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    quote_bare_rhs_tokens(&out)
}

fn parse_identifier(s: &str) -> Option<(String, usize)> {
    let mut end = 0;
    for (idx, ch) in s.char_indices() {
        if idx == 0 {
            if !ch.is_ascii_alphabetic() && ch != '_' {
                return None;
            }
        } else if !ch.is_ascii_alphanumeric() && ch != '_' {
            break;
        }
        end = idx + ch.len_utf8();
    }
    if end == 0 {
        return None;
    }
    Some((s[..end].to_string(), end))
}

/// 将 `== pending` 中的 bare word 转为 `== "pending"`（Rhai 字符串字面量）。
fn quote_bare_rhs_tokens(expr: &str) -> String {
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some(idx) = expr.find(op) {
            let head = &expr[..idx + op.len()];
            let rhs = expr[idx + op.len()..].trim();
            if !rhs.is_empty()
                && !rhs.starts_with('"')
                && !rhs.starts_with('\'')
                && rhs.parse::<f64>().is_err()
                && !matches!(rhs, "true" | "false")
                && !rhs.contains("&&")
                && !rhs.contains("||")
            {
                return format!("{head} \"{rhs}\"");
            }
            break;
        }
    }
    expr.to_string()
}

/// 从表达式中提取 `Local.*` / `System.*` 引用的 state key。
pub fn extract_state_keys(expr: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for (prefix, sys) in [("Local.", false), ("System.", true)] {
        let mut search = expr;
        while let Some(idx) = search.find(prefix) {
            let after = &search[idx + prefix.len()..];
            if let Some((key, _)) = parse_identifier(after) {
                let full = if sys { format!("sys_{key}") } else { key };
                if !keys.contains(&full) {
                    keys.push(full);
                }
            }
            search = &search[idx + prefix.len()..];
        }
    }
    keys
}

/// 从表达式中提取 `state("key")` / `local("key")` 引用的 state key。
pub fn extract_dynamic_state_keys(expr: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for prefix in ["state(", "local("] {
        let mut search = expr;
        while let Some(idx) = search.find(prefix) {
            let after = &search[idx + prefix.len()..];
            if let Some(key) = parse_quoted_key(after) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            search = &search[idx + prefix.len()..];
        }
    }
    keys
}

fn parse_quoted_key(s: &str) -> Option<String> {
    let s = s.trim_start();
    let quote = s.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &s[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

const RHAI_KEYWORDS: &[&str] = &[
    "true", "false", "and", "or", "not", "if", "else", "local", "state", "env",
];

/// 从表达式中提取可能的 bare 变量名（如 `count < 5` 中的 `count`）。
pub fn extract_bare_identifiers(expr: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut i = 0;
    let bytes = expr.as_bytes();
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c.is_ascii_alphanumeric() || c == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &expr[start..i];
            if !RHAI_KEYWORDS.contains(&ident)
                && !keys.iter().any(|k| k == ident)
            {
                keys.push(ident.to_string());
            }
        } else {
            i += 1;
        }
    }
    keys
}

/// 合并 static / dynamic / bare 引用的 state key。
pub fn extract_all_state_keys(expr: &str) -> Vec<String> {
    let mut keys = extract_state_keys(expr);
    for key in extract_dynamic_state_keys(expr) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    for key in extract_bare_identifiers(expr) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

/// 用动态 state 读取器求值布尔表达式（workflow 条件推荐路径）。
pub fn eval_workflow_bool_with_reader(
    expr: &str,
    reader: impl Fn(&str) -> Option<serde_json::Value> + Send + Sync + 'static,
) -> Result<bool> {
    let expr = expr.trim().strip_prefix('=').unwrap_or(expr).trim();
    if expr.is_empty() {
        return Ok(false);
    }

    let normalized = normalize_workflow_expr(expr);
    let mut runtime = RhaiRuntime::new();
    let reader: std::sync::Arc<dyn Fn(&str) -> Option<serde_json::Value> + Send + Sync> =
        std::sync::Arc::new(reader);

    for key in extract_all_state_keys(expr) {
        if let Some(value) = reader(key.as_str()) {
            runtime.with_json_variable(&key, value);
        }
    }

    runtime.with_dynamic_state(std::sync::Arc::clone(&reader));

    let result = runtime.eval_expression(&normalized)?;
    Ok(dynamic_to_bool(&result))
}

/// 用 workflow state 快照求值布尔表达式。
pub fn eval_workflow_bool(expr: &str, state: &HashMap<String, serde_json::Value>) -> Result<bool> {
    let expr = expr.trim().strip_prefix('=').unwrap_or(expr).trim();
    if expr.is_empty() {
        return Ok(false);
    }

    let normalized = normalize_workflow_expr(expr);
    let mut runtime = RhaiRuntime::new();
    runtime.with_workflow_state(state.clone());

    let state = state.clone();
    runtime.with_dynamic_state(std::sync::Arc::new(move |key| state.get(key).cloned()));

    let result = runtime.eval_expression(&normalized)?;
    Ok(dynamic_to_bool(&result))
}

fn dynamic_to_bool(value: &Dynamic) -> bool {
    if let Ok(b) = value.as_bool() {
        return b;
    }
    if let Ok(i) = value.as_int() {
        return i != 0;
    }
    if let Ok(f) = value.as_float() {
        return f != 0.0;
    }
    if value.is_unit() {
        return false;
    }
    let json = dynamic_to_json_val(value);
    match json {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        serde_json::Value::String(s) => matches!(s.to_lowercase().as_str(), "true" | "yes" | "1"),
        serde_json::Value::Null => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_local_reference() {
        assert_eq!(
            normalize_workflow_expr("Local.status == pending"),
            "status == \"pending\""
        );
    }

    #[test]
    fn eval_bool_with_state() {
        let mut state = HashMap::new();
        state.insert("count".into(), serde_json::json!(3));
        assert!(eval_workflow_bool("count < 5", &state).unwrap());
        assert!(!eval_workflow_bool("count > 10", &state).unwrap());
    }

    #[test]
    fn eval_local_reference() {
        let mut state = HashMap::new();
        state.insert("status".into(), serde_json::json!("pending"));
        assert!(eval_workflow_bool("Local.status == pending", &state).unwrap());
    }

    #[test]
    fn eval_dynamic_state_fn() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag_ref = std::sync::Arc::clone(&flag);
        assert!(!eval_workflow_bool_with_reader("local(\"flag\")", move |key| {
            if key == "flag" {
                Some(serde_json::json!(flag_ref.load(std::sync::atomic::Ordering::Relaxed)))
            } else {
                None
            }
        })
        .unwrap());
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
        let flag_ref = std::sync::Arc::clone(&flag);
        assert!(eval_workflow_bool_with_reader("local(\"flag\")", move |key| {
            if key == "flag" {
                Some(serde_json::json!(flag_ref.load(std::sync::atomic::Ordering::Relaxed)))
            } else {
                None
            }
        })
        .unwrap());
    }

    #[test]
    fn extract_dynamic_keys() {
        assert_eq!(
            extract_dynamic_state_keys(r#"state("count") > 0 && local("done")"#),
            vec!["count".to_string(), "done".to_string()]
        );
    }
}

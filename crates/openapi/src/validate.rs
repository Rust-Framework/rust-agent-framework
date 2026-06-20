//! OpenAPI 响应 JSON Schema 校验（feature `validate`）。

use serde_json::Value;

/// 校验 JSON 响应体是否符合 schema；无 schema 时跳过。
pub fn validate_response_body(schema: &Option<Value>, body: &str) -> Option<String> {
    #[cfg(feature = "validate")]
    {
        let schema = schema.as_ref()?;
        let instance: Value = serde_json::from_str(body).ok()?;
        let validator = jsonschema::validator_for(schema).ok()?;
        let errors: Vec<String> = validator
            .iter_errors(&instance)
            .map(|e| e.to_string())
            .collect();
        if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        }
    }
    #[cfg(not(feature = "validate"))]
    {
        let _ = (schema, body);
        None
    }
}

#[cfg(all(test, feature = "validate"))]
mod tests {
    use super::*;

    #[test]
    fn validates_matching_object() {
        let schema = serde_json::json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]});
        let err = validate_response_body(&Some(schema), r#"{"id": 1}"#);
        assert!(err.is_none());
    }

    #[test]
    fn rejects_invalid_object() {
        let schema = serde_json::json!({"type": "object", "required": ["id"]});
        let err = validate_response_body(&Some(schema), r#"{}"#);
        assert!(err.is_some());
    }
}

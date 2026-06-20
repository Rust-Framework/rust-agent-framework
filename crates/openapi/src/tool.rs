//! OpenAPI 操作 → HTTP `ITool` 实现。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{AgentError, ITool, Result, ToolResult};

use crate::spec::{
    base_url, build_request_url, parse_spec, resolve_operation, OpenApiSpec, ResolvedOperation,
    SecurityScheme,
};

/// 由 OpenAPI 操作解析的 HTTP 工具。
pub struct OpenApiHttpTool {
    name: String,
    description: String,
    method: String,
    path_template: String,
    base_url: String,
    parameters: serde_json::Value,
    operation_params: Vec<crate::spec::ResolvedParameter>,
    security: Vec<SecurityScheme>,
    response_schema: Option<serde_json::Value>,
    client: reqwest::Client,
}

impl OpenApiHttpTool {
    pub fn from_parts(
        name: impl Into<String>,
        base: &str,
        op: &ResolvedOperation,
    ) -> Self {
        let mut description = op.summary.clone();
        if let Some(schema) = &op.response_schema {
            description.push_str("\n\nResponse JSON Schema:\n```json\n");
            description.push_str(
                &serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string()),
            );
            description.push_str("\n```");
        }
        Self {
            name: name.into(),
            description,
            method: op.method.clone(),
            path_template: op.path.clone(),
            base_url: base.trim_end_matches('/').to_string(),
            parameters: op.parameters_schema.clone(),
            operation_params: op.parameters.clone(),
            security: op.security.clone(),
            response_schema: op.response_schema.clone(),
            client: reqwest::Client::new(),
        }
    }

    pub fn from_spec(
        name: impl Into<String>,
        spec: &OpenApiSpec,
        operation_id: Option<&str>,
        base_override: Option<&str>,
    ) -> std::result::Result<Self, String> {
        let tool_name = name.into();
        let op = resolve_operation(spec, operation_id, &tool_name)?;
        let base = base_url(spec, base_override);
        Ok(Self::from_parts(tool_name, &base, &op))
    }

    fn split_arguments(&self, arguments: serde_json::Value) -> (HashMap<String, String>, HashMap<String, String>, HashMap<String, String>, serde_json::Value) {
        let mut path_args = HashMap::new();
        let mut query_args = HashMap::new();
        let mut header_args = HashMap::new();
        let mut body = arguments.clone();

        if let Some(obj) = body.as_object_mut() {
            for param in &self.operation_params {
                let Some(val) = obj.remove(&param.name) else {
                    continue;
                };
                let text = json_to_string(&val);
                match param.location.as_str() {
                    "path" => {
                        path_args.insert(param.name.clone(), text);
                    }
                    "header" => {
                        header_args.insert(param.name.clone(), text);
                    }
                    "query" => {
                        query_args.insert(param.name.clone(), text);
                    }
                    _ => {
                        obj.insert(param.name.clone(), val);
                    }
                }
            }

            for key in ["bearer_token", "authorization"] {
                if let Some(val) = obj.remove(key) {
                    header_args.insert("Authorization".into(), format_bearer(&val));
                }
            }
        }

        (path_args, query_args, header_args, body)
    }

    fn apply_security(&self, headers: &mut HashMap<String, String>) {
        for scheme in &self.security {
            match scheme {
                SecurityScheme::Bearer if !headers.contains_key("Authorization") => {
                    if let Ok(token) = std::env::var("OPENAPI_BEARER_TOKEN") {
                        headers.insert("Authorization".into(), format!("Bearer {token}"));
                    }
                }
                SecurityScheme::ApiKey { name, location } if location == "header" => {
                    if !headers.contains_key(name) {
                        let env_key = format!("OPENAPI_API_KEY_{}", name.to_uppercase().replace('-', "_"));
                        if let Ok(value) = std::env::var(&env_key) {
                            headers.insert(name.clone(), value);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn json_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn format_bearer(value: &serde_json::Value) -> String {
    let raw = json_to_string(value);
    if raw.to_ascii_lowercase().starts_with("bearer ") {
        raw
    } else {
        format!("Bearer {raw}")
    }
}

#[async_trait]
impl ITool for OpenApiHttpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    fn kind(&self) -> &str {
        "openapi"
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult> {
        let (path_args, query_args, mut header_args, body) = self.split_arguments(arguments);
        self.apply_security(&mut header_args);

        let url = build_request_url(&self.base_url, &self.path_template, &path_args);

        let mut req = match self.method.as_str() {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PUT" => self.client.put(&url),
            "PATCH" => self.client.patch(&url),
            "DELETE" => self.client.delete(&url),
            other => {
                return Ok(ToolResult {
                    ok: false,
                    data: None,
                    error: Some(format!("unsupported HTTP method: {other}")),
                });
            }
        };

        if !query_args.is_empty() {
            req = req.query(&query_args);
        }
        for (k, v) in &header_args {
            req = req.header(k, v);
        }

        if self.method == "GET" || self.method == "DELETE" {
            if let Some(obj) = body.as_object() {
                if !obj.is_empty() {
                    req = req.query(obj);
                }
            }
        } else if body.is_object() {
            if let Some(obj) = body.as_object() {
                if !obj.is_empty() {
                    req = req.json(obj);
                }
            }
        } else if !body.is_null() {
            req = req.json(&body);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| AgentError::ToolError(format!("OpenAPI HTTP: {e}")))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AgentError::ToolError(format!("OpenAPI read body: {e}")))?;

        let validation_error = crate::validate::validate_response_body(&self.response_schema, &text);

        let data = serde_json::json!({
            "status": status.as_u16(),
            "body": text,
            "url": url,
            "response_schema": self.response_schema,
            "schema_valid": validation_error.is_none(),
            "schema_error": validation_error,
        });

        Ok(ToolResult {
            ok: status.is_success() && validation_error.is_none(),
            data: Some(data),
            error: if status.is_success() {
                None
            } else {
                Some(format!("HTTP {}", status.as_u16()))
            },
        })
    }
}

/// 从 spec JSON 字符串构建工具。
pub fn tool_from_spec_json(
    name: impl Into<String>,
    spec_json: &str,
    operation_id: Option<&str>,
    base_url: Option<&str>,
) -> Result<Arc<dyn ITool>> {
    let spec = parse_spec(spec_json).map_err(AgentError::ConfigError)?;
    let tool = OpenApiHttpTool::from_spec(name, &spec, operation_id, base_url)
        .map_err(AgentError::ConfigError)?;
    Ok(Arc::new(tool))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tool_from_sample_spec() {
        let spec_json = r#"{
            "openapi": "3.0.0",
            "servers": [{"url": "https://api.example.com/v1"}],
            "paths": {
                "/pets": {
                    "get": {
                        "operationId": "listPets",
                        "summary": "List all pets",
                        "parameters": [{"name": "limit", "in": "query", "schema": {"type": "integer"}}]
                    }
                }
            }
        }"#;
        let spec = crate::spec::parse_spec(spec_json).unwrap();
        let tool = OpenApiHttpTool::from_spec("listPets", &spec, Some("listPets"), None).unwrap();
        assert_eq!(tool.name(), "listPets");
        assert!(tool.parameters().get("properties").is_some());
    }

    #[test]
    fn splits_path_and_query_params() {
        let spec_json = r#"{
            "openapi": "3.0.0",
            "servers": [{"url": "https://api.example.com"}],
            "paths": {
                "/pets/{petId}": {
                    "get": {
                        "operationId": "getPet",
                        "parameters": [
                            {"name": "petId", "in": "path", "required": true, "schema": {"type": "integer"}},
                            {"name": "fields", "in": "query", "schema": {"type": "string"}}
                        ]
                    }
                }
            }
        }"#;
        let spec = crate::spec::parse_spec(spec_json).unwrap();
        let tool = OpenApiHttpTool::from_spec("getPet", &spec, Some("getPet"), None).unwrap();
        let (path, query, headers, _) = tool.split_arguments(serde_json::json!({
            "petId": 7,
            "fields": "name"
        }));
        assert_eq!(path.get("petId").map(String::as_str), Some("7"));
        assert_eq!(query.get("fields").map(String::as_str), Some("name"));
        assert!(headers.is_empty());
        assert_eq!(
            build_request_url("https://api.example.com", "/pets/{petId}", &path),
            "https://api.example.com/pets/7"
        );
    }
}

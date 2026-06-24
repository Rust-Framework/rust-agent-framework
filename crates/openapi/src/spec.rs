//! 最小 OpenAPI 3.x 规范解析（无额外 schema 库）。

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct OpenApiSpec {
    #[serde(default)]
    pub openapi: String,
    #[serde(default)]
    pub servers: Vec<Server>,
    pub paths: Option<Value>,
    #[serde(default)]
    pub components: Option<Components>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Components {
    #[serde(default)]
    pub schemas: HashMap<String, Value>,
    #[serde(default)]
    pub parameters: HashMap<String, Value>,
    #[serde(default, rename = "securitySchemes")]
    pub security_schemes: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedParameter {
    pub name: String,
    pub location: String,
    pub schema: Value,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub enum SecurityScheme {
    Bearer,
    ApiKey { name: String, location: String },
}

#[derive(Debug, Clone)]
pub struct ResolvedOperation {
    pub method: String,
    pub path: String,
    pub operation_id: String,
    pub summary: String,
    pub parameters: Vec<ResolvedParameter>,
    pub parameters_schema: Value,
    pub response_schema: Option<Value>,
    pub security: Vec<SecurityScheme>,
}

pub fn parse_spec(raw: &str) -> Result<OpenApiSpec, String> {
    serde_json::from_str(raw).map_err(|e| format!("invalid OpenAPI JSON: {e}"))
}

pub fn resolve_operation(
    spec: &OpenApiSpec,
    operation_id: Option<&str>,
    tool_name: &str,
) -> Result<ResolvedOperation, String> {
    let paths = spec
        .paths
        .as_ref()
        .ok_or("OpenAPI spec has no paths")?;

    let obj = paths
        .as_object()
        .ok_or("OpenAPI paths must be an object")?;

    for (path, item) in obj {
        let item_obj = item
            .as_object()
            .ok_or_else(|| format!("invalid path item for {path}"))?;
        for method in ["get", "post", "put", "patch", "delete"] {
            let Some(op_val) = item_obj.get(method) else {
                continue;
            };
            let op_obj = op_val
                .as_object()
                .ok_or_else(|| format!("invalid operation at {method} {path}"))?;
            let op_id = op_obj
                .get("operationId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(want) = operation_id {
                if op_id != want {
                    continue;
                }
            }

            let summary = op_obj
                .get("summary")
                .or_else(|| op_obj.get("description"))
                .and_then(|v| v.as_str())
                .unwrap_or(tool_name)
                .to_string();

            let parameters = collect_parameters(spec, op_obj);
            let parameters_schema = build_parameters_schema(&parameters, spec, op_obj);
            let response_schema = resolve_response_schema(spec, op_obj);
            let security = resolve_operation_security(spec, op_obj);

            return Ok(ResolvedOperation {
                method: method.to_uppercase(),
                path: path.clone(),
                operation_id: if op_id.is_empty() {
                    format!("{method}_{path}")
                } else {
                    op_id
                },
                summary,
                parameters,
                parameters_schema,
                response_schema,
                security,
            });
        }
    }

    Err(format!(
        "no matching operation found (operationId={operation_id:?})"
    ))
}

fn collect_parameters(spec: &OpenApiSpec, op: &serde_json::Map<String, Value>) -> Vec<ResolvedParameter> {
    let mut out = Vec::new();
    if let Some(params) = op.get("parameters").and_then(|p| p.as_array()) {
        for p in params {
            let pobj = match resolve_parameter(spec, p) {
                Some(v) => v,
                None => continue,
            };
            let Some(pobj) = pobj.as_object() else { continue };
            let name = pobj.get("name").and_then(|n| n.as_str()).unwrap_or("param").to_string();
            let location = pobj
                .get("in")
                .and_then(|v| v.as_str())
                .unwrap_or("query")
                .to_string();
            let schema = pobj
                .get("schema")
                .map(|s| resolve_schema(spec, s))
                .unwrap_or_else(|| serde_json::json!({"type":"string"}));
            let required = pobj.get("required").and_then(|v| v.as_bool()).unwrap_or(false);
            out.push(ResolvedParameter {
                name,
                location,
                schema,
                required,
            });
        }
    }
    out
}

fn build_parameters_schema(
    path_params: &[ResolvedParameter],
    spec: &OpenApiSpec,
    op: &serde_json::Map<String, Value>,
) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for p in path_params {
        properties.insert(p.name.clone(), p.schema.clone());
        if p.required {
            required.push(p.name.clone());
        }
    }

    if let Some(params) = op.get("parameters").and_then(|p| p.as_array()) {
        for p in params {
            let pobj = match resolve_parameter(spec, p) {
                Some(v) => v,
                None => continue,
            };
            let Some(pobj) = pobj.as_object() else { continue };
            let name = pobj.get("name").and_then(|n| n.as_str()).unwrap_or("param");
            if properties.contains_key(name) {
                continue;
            }
            let schema = pobj
                .get("schema")
                .map(|s| resolve_schema(spec, s))
                .unwrap_or_else(|| serde_json::json!({"type":"string"}));
            properties.insert(name.to_string(), schema);
            if pobj.get("required").and_then(|v| v.as_bool()).unwrap_or(false) {
                required.push(name.to_string());
            }
        }
    }

    if let Some(body) = op.get("requestBody") {
        let body = resolve_value_ref(spec, body);
        if let Some(content) = body.get("content").and_then(|c| c.as_object()) {
            for (_mime, media) in content {
                if let Some(schema) = media.get("schema") {
                    let schema = resolve_schema(spec, schema);
                    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                        for (k, v) in props {
                            properties.insert(k.clone(), resolve_schema(spec, v));
                        }
                    }
                    if let Some(req) = schema.get("required").and_then(|r| r.as_array()) {
                        for r in req {
                            if let Some(s) = r.as_str() {
                                required.push(s.to_string());
                            }
                        }
                    }
                    break;
                }
            }
        }
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn resolve_parameter(spec: &OpenApiSpec, param: &Value) -> Option<Value> {
    if param.get("$ref").is_some() {
        resolve_value_ref(spec, param).into()
    } else {
        Some(param.clone())
    }
}

/// 解析 `#/components/schemas/*` 与 `#/components/parameters/*` 引用。
pub fn resolve_value_ref(spec: &OpenApiSpec, value: &Value) -> Value {
    let Some(ref_path) = value.get("$ref").and_then(|v| v.as_str()) else {
        return value.clone();
    };

    let resolved = ref_path
        .strip_prefix("#/components/")
        .and_then(|rest| rest.split_once('/'))
        .and_then(|(kind, name)| {
            spec.components.as_ref().and_then(|c| match kind {
                "schemas" => c.schemas.get(name).cloned(),
                "parameters" => c.parameters.get(name).cloned(),
                _ => None,
            })
        });

    resolved
        .map(|v| resolve_value_ref(spec, &v))
        .unwrap_or_else(|| value.clone())
}

/// 解析 JSON Schema，展开 `$ref` 并合并 `allOf` 顶层属性。
pub fn resolve_schema(spec: &OpenApiSpec, schema: &Value) -> Value {
    let schema = resolve_value_ref(spec, schema);

    if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
        let mut merged = serde_json::Map::new();
        let mut required = Vec::new();
        for part in all_of {
            let part = resolve_schema(spec, part);
            if let Some(props) = part.get("properties").and_then(|p| p.as_object()) {
                for (k, v) in props {
                    merged.insert(k.clone(), v.clone());
                }
            }
            if let Some(req) = part.get("required").and_then(|r| r.as_array()) {
                for r in req {
                    if let Some(s) = r.as_str() {
                        required.push(s.to_string());
                    }
                }
            }
        }
        if let Some(obj) = schema.as_object() {
            let mut out = obj.clone();
            if !merged.is_empty() {
                out.insert("properties".into(), Value::Object(merged));
            }
            if !required.is_empty() {
                out.insert("required".into(), serde_json::json!(required));
            }
            out.remove("allOf");
            return Value::Object(out);
        }
    }

    schema
}

fn deep_resolve_schema(spec: &OpenApiSpec, schema: &Value) -> Value {
    let schema = resolve_schema(spec, schema);
    let Some(obj) = schema.as_object() else {
        return schema;
    };
    let mut out = obj.clone();
    if let Some(props) = out.get_mut("properties").and_then(|p| p.as_object_mut()) {
        for (_, v) in props.iter_mut() {
            *v = deep_resolve_schema(spec, v);
        }
    }
    if let Some(items) = out.get("items") {
        out.insert("items".into(), deep_resolve_schema(spec, items));
    }
    Value::Object(out)
}

fn resolve_response_schema(spec: &OpenApiSpec, op: &serde_json::Map<String, Value>) -> Option<Value> {
    let responses = op.get("responses")?.as_object()?;
    let success = responses
        .get("200")
        .or_else(|| responses.get("201"))
        .or_else(|| responses.get("204"))?;
    let content = success.get("content")?.as_object()?;
    let media = content
        .get("application/json")
        .or_else(|| content.values().next())?;
    media
        .get("schema")
        .map(|s| deep_resolve_schema(spec, s))
}

fn resolve_operation_security(spec: &OpenApiSpec, op: &serde_json::Map<String, Value>) -> Vec<SecurityScheme> {
    let Some(entries) = op.get("security").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut schemes = Vec::new();
    for entry in entries {
        let Some(obj) = entry.as_object() else { continue };
        for name in obj.keys() {
            if let Some(scheme) = resolve_security_scheme(spec, name) {
                if !schemes.iter().any(|s| security_eq(s, &scheme)) {
                    schemes.push(scheme);
                }
            }
        }
    }
    schemes
}

fn security_eq(a: &SecurityScheme, b: &SecurityScheme) -> bool {
    match (a, b) {
        (SecurityScheme::Bearer, SecurityScheme::Bearer) => true,
        (
            SecurityScheme::ApiKey { name: n1, location: l1 },
            SecurityScheme::ApiKey { name: n2, location: l2 },
        ) => n1 == n2 && l1 == l2,
        _ => false,
    }
}

fn resolve_security_scheme(spec: &OpenApiSpec, name: &str) -> Option<SecurityScheme> {
    let scheme = spec
        .components
        .as_ref()?
        .security_schemes
        .get(name)?;
    let scheme_type = scheme.get("type")?.as_str()?;
    match scheme_type {
        "http" if scheme.get("scheme").and_then(|v| v.as_str()) == Some("bearer") => {
            Some(SecurityScheme::Bearer)
        }
        "apiKey" => Some(SecurityScheme::ApiKey {
            name: scheme.get("name")?.as_str()?.to_string(),
            location: scheme.get("in")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

/// 将 path 模板 `{id}` 替换为参数值。
pub fn build_request_url(base: &str, path: &str, path_args: &HashMap<String, String>) -> String {
    let mut url = format!("{}{}", base.trim_end_matches('/'), path);
    for (key, value) in path_args {
        url = url.replace(&format!("{{{key}}}"), value);
    }
    url
}

pub fn base_url(spec: &OpenApiSpec, override_url: Option<&str>) -> String {
    if let Some(u) = override_url {
        return u.trim_end_matches('/').to_string();
    }
    spec.servers
        .first()
        .map(|s| s.url.trim_end_matches('/').to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "openapi": "3.0.0",
        "servers": [{"url": "https://api.example.com"}],
        "paths": {
            "/pets": {
                "get": {
                    "operationId": "listPets",
                    "summary": "List pets",
                    "parameters": [
                        {"name": "limit", "in": "query", "schema": {"type": "integer"}}
                    ]
                }
            }
        }
    }"#;

    const REF_SAMPLE: &str = r##"{
        "openapi": "3.0.0",
        "components": {
            "schemas": {
                "PetId": {"type": "integer", "minimum": 1}
            },
            "parameters": {
                "LimitParam": {
                    "name": "limit",
                    "in": "query",
                    "schema": {"$ref": "#/components/schemas/PetId"}
                }
            }
        },
        "paths": {
            "/pets/{id}": {
                "get": {
                    "operationId": "getPet",
                    "parameters": [{"$ref": "#/components/parameters/LimitParam"}]
                }
            }
        }
    }"##;

    #[test]
    fn resolves_operation_by_id() {
        let spec = parse_spec(SAMPLE).unwrap();
        let op = resolve_operation(&spec, Some("listPets"), "listPets").unwrap();
        assert_eq!(op.method, "GET");
        assert_eq!(op.path, "/pets");
    }

    #[test]
    fn resolves_parameter_schema_ref() {
        let spec = parse_spec(REF_SAMPLE).unwrap();
        let op = resolve_operation(&spec, Some("getPet"), "getPet").unwrap();
        let limit = op
            .parameters_schema
            .pointer("/properties/limit")
            .expect("limit property");
        assert_eq!(limit.get("type").and_then(|v| v.as_str()), Some("integer"));
        assert_eq!(limit.get("minimum").and_then(|v| v.as_i64()), Some(1));
    }

    #[test]
    fn resolves_response_schema() {
        let spec = parse_spec(r#"{
            "openapi": "3.0.0",
            "paths": {
                "/items": {
                    "get": {
                        "operationId": "listItems",
                        "responses": {
                            "200": {
                                "description": "OK",
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "type": "array",
                                            "items": {"type": "string"}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }"#)
        .unwrap();
        let op = resolve_operation(&spec, Some("listItems"), "listItems").unwrap();
        assert_eq!(
            op.response_schema
                .as_ref()
                .and_then(|s| s.get("type"))
                .and_then(|v| v.as_str()),
            Some("array")
        );
    }

    #[test]
    fn resolves_bearer_security() {
        let spec = parse_spec(r##"{
            "openapi": "3.0.0",
            "components": {
                "securitySchemes": {
                    "bearerAuth": {"type": "http", "scheme": "bearer"}
                }
            },
            "paths": {
                "/secure": {
                    "get": {
                        "operationId": "secureOp",
                        "security": [{"bearerAuth": []}]
                    }
                }
            }
        }"##)
        .unwrap();
        let op = resolve_operation(&spec, Some("secureOp"), "secureOp").unwrap();
        assert!(matches!(op.security.as_slice(), [SecurityScheme::Bearer]));
    }

    #[test]
    fn build_request_url_replaces_path_params() {
        let mut args = HashMap::new();
        args.insert("id".into(), "42".into());
        assert_eq!(
            build_request_url("https://x.com", "/items/{id}", &args),
            "https://x.com/items/42"
        );
    }
}

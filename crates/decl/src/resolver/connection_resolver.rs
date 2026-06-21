use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_client::{ChatClientOptions, DeepSeekChatClient, OpenAiChatClient};
use rust_agent_core::IChatClient;

use crate::connection::{Connection, ConnectionKind};
use crate::error::DeclError;
use crate::model::Model;

/// 将 `Model` + `Connection` 解析为 `Arc<dyn IChatClient>`。
///
/// 支持以下连接类型：
/// - `ApiKey` — 直接 API 密钥认证
/// - `Anonymous` — 无需认证
/// - `Remote` — 使用端点，基于环境的凭据
/// - `Reference` — 按名称解析 `connections` 注册表中的连接
/// - `OAuth` — 从环境变量读取 Bearer token（`tokenEnvVar`，默认 `OAUTH_ACCESS_TOKEN`）
pub fn resolve_chat_client(model: &Model) -> crate::Result<Arc<dyn IChatClient>> {
    resolve_chat_client_with_registry(model, None)
}

/// 带命名连接注册表的解析（用于 `kind: reference`）。
pub fn resolve_chat_client_with_registry(
    model: &Model,
    connections: Option<&HashMap<String, Connection>>,
) -> crate::Result<Arc<dyn IChatClient>> {
    let connection = model.connection.as_ref();
    let provider = model
        .provider
        .as_deref()
        .unwrap_or("openai")
        .to_lowercase();

    let (api_key, base_url) = match connection {
        Some(conn) => extract_credentials(conn, connections, 0)?,
        None => {
            return Err(DeclError::Missing(
                "connection is required in model config".into(),
            ));
        }
    };

    let api_key = api_key.ok_or_else(|| {
        DeclError::Missing("api_key is required for model connection".into())
    })?;

    let mut options = match provider.as_str() {
        "openai" => ChatClientOptions::openai(&model.id, api_key),
        "deepseek" => ChatClientOptions::deepseek(&model.id, api_key),
        "custom" => ChatClientOptions {
            api_base: base_url
                .clone()
                .ok_or_else(|| DeclError::Missing("base_url required for custom provider".into()))?,
            api_key,
            model: model.id.clone(),
            ..Default::default()
        },
        other => {
            return Err(DeclError::Unsupported(format!(
                "Unknown provider '{}'. Supported: openai, custom",
                other
            )));
        }
    };

    // Apply model options
    if let Some(opts) = &model.options {
        if let Some(temp) = opts.temperature {
            options.temperature = Some(temp as f32);
        }
        if let Some(mt) = opts.max_output_tokens {
            options.max_tokens = Some(mt as u32);
        }
        let _ = opts.seed;
    }

    // Apply extra headers from connection details
    if let Some(conn) = connection {
        for (k, v) in &conn.details.extra {
            if let Some(val_str) = v.as_str() {
                options.extra_headers.insert(k.clone(), val_str.to_string());
            }
        }
    }

    match provider.as_str() {
        "openai" => Ok(Arc::new(OpenAiChatClient::new(options)?)),
        "deepseek" => Ok(Arc::new(DeepSeekChatClient::new(options)?)),
        "custom" => Ok(Arc::new(OpenAiChatClient::new(options)?)),
        other => Err(DeclError::Unsupported(format!(
            "Unknown provider: {}",
            other
        ))),
    }
}

const MAX_REFERENCE_DEPTH: usize = 8;

/// 从 Connection 提取 API 密钥和可选的 base URL。
fn extract_credentials(
    conn: &Connection,
    connections: Option<&HashMap<String, Connection>>,
    depth: usize,
) -> crate::Result<(Option<String>, Option<String>)> {
    if depth > MAX_REFERENCE_DEPTH {
        return Err(DeclError::Resolution(
            "Connection reference chain too deep (max 8)".into(),
        ));
    }

    match conn.kind {
        ConnectionKind::ApiKey | ConnectionKind::Foundry => {
            let api_key = conn
                .details
                .api_key
                .clone()
                .or_else(|| conn.details.extra.get("apiKey").and_then(|v| v.as_str().map(String::from)));

            let resolved_key = match api_key {
                Some(key) if key.starts_with('$') => {
                    let var_name = &key[1..];
                    std::env::var(var_name).map_err(|_| {
                        DeclError::Resolution(format!(
                            "Environment variable '{}' not set (referenced by connection api_key)",
                            var_name
                        ))
                    })?
                }
                Some(key) if key.starts_with("=Env.") => {
                    let var_name = &key[5..];
                    std::env::var(var_name).map_err(|_| {
                        DeclError::Resolution(format!(
                            "Environment variable '{}' not set (=Env.{} in connection)",
                            var_name, var_name
                        ))
                    })?
                }
                Some(key) => key,
                None => {
                    return Err(DeclError::Missing("api_key not found in connection".into()));
                }
            };

            let base_url = conn.details.endpoint.clone();
            Ok((Some(resolved_key), base_url))
        }
        ConnectionKind::Remote => {
            let endpoint = conn
                .details
                .endpoint
                .clone()
                .ok_or_else(|| DeclError::Missing("endpoint required for remote connection".into()))?;
            let key_var = conn
                .details
                .extra
                .get("keyEnvVar")
                .and_then(|v| v.as_str())
                .unwrap_or("AZURE_OPENAI_API_KEY");
            let api_key = std::env::var(key_var).map_err(|_| {
                DeclError::Resolution(format!(
                    "Environment variable '{}' not set for remote connection",
                    key_var
                ))
            })?;
            Ok((Some(api_key), Some(endpoint)))
        }
        ConnectionKind::Anonymous => Ok((Some(String::new()), None)),
        ConnectionKind::Reference => {
            let ref_name = conn
                .details
                .name
                .as_deref()
                .or(conn.details.target.as_deref())
                .ok_or_else(|| {
                    DeclError::Missing(
                        "Reference connection requires `name` or `target` field".into(),
                    )
                })?;
            let target = connections
                .and_then(|m| m.get(ref_name))
                .ok_or_else(|| {
                    DeclError::Resolution(format!(
                        "Reference connection '{ref_name}' not found. \
                         Register via DeclAgentBuilder::with_connection()."
                    ))
                })?;
            extract_credentials(target, connections, depth + 1)
        }
        ConnectionKind::OAuth => {
            let token_var = conn
                .details
                .extra
                .get("tokenEnvVar")
                .and_then(|v| v.as_str())
                .unwrap_or("OAUTH_ACCESS_TOKEN");
            let token = std::env::var(token_var).map_err(|_| {
                DeclError::Resolution(format!(
                    "Environment variable '{token_var}' not set for OAuth connection"
                ))
            })?;
            Ok((Some(token), conn.details.endpoint.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::{AuthenticationMode, ConnectionDetails};

    #[test]
    fn reference_connection_resolves_via_registry() {
        let mut connections = HashMap::new();
        connections.insert(
            "my-openai".into(),
            Connection {
                kind: ConnectionKind::ApiKey,
                authentication_mode: AuthenticationMode::System,
                usage_description: None,
                details: ConnectionDetails {
                    api_key: Some("sk-test-key".into()),
                    ..Default::default()
                },
            },
        );

        let model = Model {
            id: "gpt-4o".into(),
            provider: Some("openai".into()),
            connection: Some(Connection {
                kind: ConnectionKind::Reference,
                authentication_mode: AuthenticationMode::System,
                usage_description: None,
                details: ConnectionDetails {
                    name: Some("my-openai".into()),
                    ..Default::default()
                },
            }),
            options: None,
            api_type: None,
        };

        let client = resolve_chat_client_with_registry(&model, Some(&connections))
            .expect("reference resolves");
        assert_eq!(client.model_id(), "gpt-4o");
    }
}

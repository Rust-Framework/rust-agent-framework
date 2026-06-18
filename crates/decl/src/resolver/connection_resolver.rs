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
pub fn resolve_chat_client(model: &Model) -> crate::Result<Arc<dyn IChatClient>> {
    let connection = model.connection.as_ref();
    let provider = model
        .provider
        .as_deref()
        .unwrap_or("openai")
        .to_lowercase();

    let (api_key, base_url) = match connection {
        Some(conn) => extract_credentials(conn)?,
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
                "Unknown provider '{}'. Supported: openai, deepseek, custom",
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
        // Note: seed is not directly supported by ChatClientOptions.
        // Use `extra` for provider-specific options not in the public API.
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
            "Unknown provider in build_client: {}",
            other
        ))),
    }
}

/// 从 Connection 提取 API 密钥和可选的 base URL。
fn extract_credentials(conn: &Connection) -> crate::Result<(Option<String>, Option<String>)> {
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
                    let var_name = &key[5..]; // strip "=Env."
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
            // For remote connections, the API key must come from environment
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
        ConnectionKind::Anonymous => {
            // No auth required — use empty key
            Ok((Some(String::new()), None))
        }
        ConnectionKind::Reference => {
            Err(DeclError::Unsupported(
                "Reference connections not yet supported in Rust resolver".into(),
            ))
        }
        ConnectionKind::OAuth => {
            Err(DeclError::Unsupported(
                "OAuth connections not yet supported in Rust resolver".into(),
            ))
        }
    }
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// AI 模型的连接配置，与 MAF AgentSchema v1.0 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub kind: ConnectionKind,
    #[serde(default = "default_authentication_mode")]
    pub authentication_mode: AuthenticationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_description: Option<String>,
    /// 类型特定的连接详情（endpoint、api_key 等）。
    #[serde(flatten)]
    pub details: ConnectionDetails,
}

/// AI 服务的认证/连接类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// Azure 托管的远程连接，使用托管身份。
    Remote,
    /// 命名连接引用（按名称查找）。
    Reference,
    /// 基于 API 密钥的认证。
    #[serde(rename = "key")]
    ApiKey,
    /// 无需认证。
    Anonymous,
    /// Microsoft Foundry 项目连接。
    Foundry,
    /// 基于 OAuth2 的认证。
    #[serde(rename = "oauth")]
    OAuth,
}

/// 连接的权限级别。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMode {
    /// 以用户权限建立连接。
    User,
    /// 以系统/应用权限建立连接。
    System,
}

fn default_authentication_mode() -> AuthenticationMode {
    AuthenticationMode::System
}

/// 类型特定的连接字段，可扩展用于未来扩展。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDetails {
    /// 服务端点 URL（例如 Azure OpenAI 端点）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// 基于密钥的连接的 API 密钥。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// 命名连接引用目标。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 可选的目标标识符。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// 用于向前兼容的额外字段。
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Default for ConnectionDetails {
    fn default() -> Self {
        Self {
            endpoint: None,
            api_key: None,
            name: None,
            target: None,
            extra: HashMap::new(),
        }
    }
}

impl Connection {
    /// 创建 API 密钥连接。
    pub fn api_key(key: impl Into<String>) -> Self {
        Self {
            kind: ConnectionKind::ApiKey,
            authentication_mode: AuthenticationMode::System,
            usage_description: None,
            details: ConnectionDetails {
                api_key: Some(key.into()),
                ..Default::default()
            },
        }
    }

    /// 创建匿名连接（无需认证）。
    pub fn anonymous() -> Self {
        Self {
            kind: ConnectionKind::Anonymous,
            authentication_mode: AuthenticationMode::System,
            usage_description: None,
            details: ConnectionDetails::default(),
        }
    }

    /// 用给定端点创建远程连接。
    pub fn remote(endpoint: impl Into<String>) -> Self {
        Self {
            kind: ConnectionKind::Remote,
            authentication_mode: AuthenticationMode::System,
            usage_description: None,
            details: ConnectionDetails {
                endpoint: Some(endpoint.into()),
                ..Default::default()
            },
        }
    }
}

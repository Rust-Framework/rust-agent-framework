use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Connection configuration for an AI model.
/// Aligns with MAF AgentSchema v1.0 `Connection`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub kind: ConnectionKind,
    #[serde(default = "default_authentication_mode")]
    pub authentication_mode: AuthenticationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_description: Option<String>,
    /// Kind-specific connection details (endpoint, api_key, etc.).
    #[serde(flatten)]
    pub details: ConnectionDetails,
}

/// The type of authentication/connection for the AI service.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// Azure-hosted remote connection with managed identity.
    Remote,
    /// Named connection reference (looked up by name).
    Reference,
    /// API key-based authentication.
    #[serde(rename = "key")]
    ApiKey,
    /// No authentication required.
    Anonymous,
    /// Microsoft Foundry project connection.
    Foundry,
    /// OAuth2-based authentication.
    #[serde(rename = "oauth")]
    OAuth,
}

/// The authority level for the connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMode {
    /// Connection made under the user's authority.
    User,
    /// Connection made under the system/application's authority.
    System,
}

fn default_authentication_mode() -> AuthenticationMode {
    AuthenticationMode::System
}

/// Kind-specific connection fields. Open-ended for future extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDetails {
    /// Service endpoint URL (e.g., Azure OpenAI endpoint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// API key for key-based connections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Named connection reference target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Optional target identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Extra fields for forward compatibility.
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
    /// Create an API key connection.
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

    /// Create an anonymous connection (no auth).
    pub fn anonymous() -> Self {
        Self {
            kind: ConnectionKind::Anonymous,
            authentication_mode: AuthenticationMode::System,
            usage_description: None,
            details: ConnectionDetails::default(),
        }
    }

    /// Create a remote connection with the given endpoint.
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

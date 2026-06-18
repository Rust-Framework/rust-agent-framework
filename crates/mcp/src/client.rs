//! MCP client — manages connection lifecycle and protocol operations.
//!
//! Implements the MCP protocol over a transport layer:
//! - Initialize handshake (capability negotiation)
//! - Tool discovery (tools/list) and execution (tools/call)
//! - Resource listing (resources/list) and reading (resources/read)
//! - Prompt listing (prompts/list) and retrieval (prompts/get)

use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::transport::{Transport, TransportConfig, TransportError, create_transport};
use crate::types::*;

/// MCP client wrapping a transport and managing the protocol conversation.
pub struct McpClient {
    transport: Box<dyn Transport>,
    server_info: Option<ServerInfo>,
    server_capabilities: Option<ServerCapabilities>,
    next_id: Mutex<i64>,
    initialized: Mutex<bool>,
}

/// MCP connection configuration: how to reach the server.
#[derive(Debug, Clone)]
pub struct McpConnectionOptions {
    pub transport: TransportConfig,
}

impl McpConnectionOptions {
    /// Create a config for a stdio subprocess MCP server (most common case).
    pub fn stdio(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            transport: TransportConfig::Stdio {
                command: command.into(),
                args,
            },
        }
    }

    /// Create a config for an HTTP SSE MCP server.
    pub fn sse(sse_url: impl Into<String>, post_url: impl Into<String>) -> Self {
        Self {
            transport: TransportConfig::Sse {
                sse_url: sse_url.into(),
                post_url: post_url.into(),
            },
        }
    }
}

impl McpClient {
    /// Connect to an MCP server, performing the initialize handshake.
    ///
    /// This is the primary constructor. It:
    /// 1. Creates the transport (stdio subprocess or SSE connection)
    /// 2. Sends `initialize` with client capabilities
    /// 3. Receives server capabilities
    /// 4. Sends `initialized` notification
    pub async fn connect(config: McpConnectionOptions) -> Result<Self, McpError> {
        let transport = create_transport(config.transport).await?;
        let mut client = Self {
            transport,
            server_info: None,
            server_capabilities: None,
            next_id: Mutex::new(1),
            initialized: Mutex::new(false),
        };

        // Perform initialize handshake
        let init_result = client.initialize().await?;
        client.server_info = Some(init_result.server_info);
        client.server_capabilities = Some(init_result.capabilities);
        *client.initialized.lock().await = true;

        // Send initialized notification
        client
            .send_notification(methods::INITIALIZED, None)
            .await?;

        tracing::info!(
            server = %client.server_info.as_ref().map(|i| &*i.name).unwrap_or("unknown"),
            version = %client.server_info.as_ref().map(|i| &*i.version).unwrap_or("unknown"),
            "MCP client connected"
        );

        Ok(client)
    }

    /// Get the server info from the initialize handshake.
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Get the server capabilities negotiated during initialize.
    pub fn server_capabilities(&self) -> Option<&ServerCapabilities> {
        self.server_capabilities.as_ref()
    }

    /// Check if the initiailize handshake has completed.
    pub fn is_initialized(&self) -> bool {
        *self.initialized.blocking_lock()
    }

    // ── Low-level RPC ─────────────────────────────────────────────────────

    async fn next_id(&self) -> JsonRpcId {
        let mut id = self.next_id.lock().await;
        let current = *id;
        *id += 1;
        JsonRpcId::Number(current)
    }

    async fn send_request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, McpError> {
        let id = self.next_id().await;
        let request = JsonRpcRequest::new(id, method, params);
        let msg = JsonRpcMessage::Request(request);

        self.transport
            .send(&msg)
            .await
            .map_err(|e| McpError::Transport(e.to_string()))?;

        // Read responses until we find the matching id
        loop {
            let response = self
                .transport
                .recv()
                .await
                .map_err(|e| McpError::Transport(e.to_string()))?
                .ok_or(McpError::Transport("Transport closed before response".into()))?;

            match response {
                JsonRpcMessage::Response(resp) => {
                    match resp {
                        JsonRpcResponse::Success { id: _, result, .. } => {
                            return Ok(result);
                        }
                        JsonRpcResponse::Error { id: _, error, .. } => {
                            return Err(McpError::Rpc {
                                code: error.code,
                                message: error.message,
                                data: error.data,
                            });
                        }
                    }
                }
                JsonRpcMessage::Notification(notif) => {
                    // Handle notifications (e.g., tools/list_changed)
                    tracing::debug!(
                        method = %notif.method,
                        "Received MCP notification while waiting for response"
                    );
                    // Continue polling for the actual response
                }
                _ => {
                    tracing::warn!("Unexpected message type while waiting for response");
                }
            }
        }
    }

    async fn send_notification(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), McpError> {
        let notif = JsonRpcNotification::new(method, params);
        let msg = JsonRpcMessage::Notification(notif);
        self.transport
            .send(&msg)
            .await
            .map_err(|e| McpError::Transport(e.to_string()))
    }

    // ── Initialize ────────────────────────────────────────────────────────

    async fn initialize(&self) -> Result<InitializeResult, McpError> {
        let params = serde_json::to_value(InitializeRequest {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities {
                roots: Some(RootsCapability::default()),
                ..Default::default()
            },
            client_info: ImplementationInfo {
                name: "rust-agent-framework".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        })?;

        let result = self.send_request(methods::INITIALIZE, Some(params)).await?;
        let init_result: InitializeResult =
            serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))?;

        // Validate protocol version compatibility
        if init_result.protocol_version != MCP_PROTOCOL_VERSION {
            tracing::warn!(
                server_version = %init_result.protocol_version,
                client_version = %MCP_PROTOCOL_VERSION,
                "MCP protocol version mismatch"
            );
        }

        Ok(init_result)
    }

    // ── Tools ─────────────────────────────────────────────────────────────

    /// List all available tools from the MCP server. Returns None if tools are not supported.
    pub async fn list_tools(&self, cursor: Option<String>) -> Result<Option<ListToolsResult>, McpError> {
        if !self.server_capabilities.as_ref().map(|c| c.tools.is_some()).unwrap_or(false) {
            return Ok(None);
        }
        let params = serde_json::to_value(ListToolsRequest { cursor })?;
        let result = self.send_request(methods::TOOLS_LIST, Some(params)).await?;
        let tools_result: ListToolsResult =
            serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(Some(tools_result))
    }

    /// Call a specific tool on the MCP server.
    pub async fn call(
        &self,
        name: &str,
        arguments: HashMap<String, serde_json::Value>,
    ) -> Result<CallToolResult, McpError> {
        let params = serde_json::to_value(CallToolRequest {
            name: name.to_string(),
            arguments: Some(arguments),
        })?;
        let result = self.send_request(methods::TOOLS_CALL, Some(params)).await?;
        let call_result: CallToolResult =
            serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(call_result)
    }

    // ── Resources ─────────────────────────────────────────────────────────

    /// List available resources. Returns None if resources are not supported.
    pub async fn list_resources(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<ListResourcesResult>, McpError> {
        if !self.server_capabilities.as_ref().map(|c| c.resources.is_some()).unwrap_or(false) {
            return Ok(None);
        }
        let params = serde_json::to_value(ListResourcesParams { cursor })?;
        let result = self.send_request(methods::RESOURCES_LIST, Some(params)).await?;
        let resources_result: ListResourcesResult =
            serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(Some(resources_result))
    }

    /// Read a specific resource by URI.
    pub async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult, McpError> {
        let params = serde_json::to_value(ReadResourceRequest {
            uri: uri.to_string(),
        })?;
        let result = self.send_request(methods::RESOURCES_READ, Some(params)).await?;
        let read_result: ReadResourceResult =
            serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(read_result)
    }

    // ── Prompts ───────────────────────────────────────────────────────────

    /// List available prompts. Returns None if prompts are not supported.
    pub async fn list_prompts(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<ListPromptsResult>, McpError> {
        if !self.server_capabilities.as_ref().map(|c| c.prompts.is_some()).unwrap_or(false) {
            return Ok(None);
        }
        let params = serde_json::to_value(ListPromptsParams { cursor })?;
        let result = self.send_request(methods::PROMPTS_LIST, Some(params)).await?;
        let prompts_result: ListPromptsResult =
            serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(Some(prompts_result))
    }

    /// Get a specific prompt by name.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<HashMap<String, String>>,
    ) -> Result<GetPromptResult, McpError> {
        let params = serde_json::to_value(GetPromptRequest {
            name: name.to_string(),
            arguments,
        })?;
        let result = self.send_request(methods::PROMPTS_GET, Some(params)).await?;
        let prompt_result: GetPromptResult =
            serde_json::from_value(result).map_err(|e| McpError::Protocol(e.to_string()))?;
        Ok(prompt_result)
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────

    /// Close the connection to the MCP server.
    pub async fn close(&self) -> Result<(), McpError> {
        self.transport
            .close()
            .await
            .map_err(|e| McpError::Transport(e.to_string()))
    }
}

// ── Error Types ───────────────────────────────────────────────────────────

/// MCP client errors.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("RPC error (code={code}): {message}")]
    Rpc {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },

    #[error("Timeout: {0}")]
    Timeout(String),
}

impl McpError {
    /// Extract text from a CallToolResult error response for user display.
    pub fn from_call_result(result: &CallToolResult) -> Self {
        let text = ToolContent::extract_text(&result.content);
        McpError::Rpc {
            code: -32000,
            message: if text.is_empty() {
                "Tool execution failed".to_string()
            } else {
                text
            },
            data: None,
        }
    }
}

impl From<TransportError> for McpError {
    fn from(e: TransportError) -> Self {
        McpError::Transport(e.to_string())
    }
}

impl From<serde_json::Error> for McpError {
    fn from(e: serde_json::Error) -> Self {
        McpError::Protocol(e.to_string())
    }
}

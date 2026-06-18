//! ITool adapter for MCP tools — bridges MCP server tools into the RAF tool ecosystem.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{ITool, ToolResult};
use crate::client::{McpClient, McpConnectionOptions};
use crate::types::ToolInfo;

/// An ITool implementation that delegates to an MCP server via `tools/call`.
///
/// Each `McpTool` wraps a single tool exposed by an MCP server. Multiple
/// `McpTool` instances can share the same `McpClient` connection.
///
/// # Example
///
/// ```ignore
/// use rust_agent_mcp::{McpClient, McpConnectionOptions, McpTool};
/// use std::collections::HashMap;
///
/// let config = McpConnectionOptions::stdio("my-mcp-server", vec![]);
/// let client = McpClient::connect(config).await?;
/// let tool_info = client.list_tools(None).await?.unwrap().tools.into_iter().next().unwrap();
///
/// let mcp_tool = McpTool::new(Arc::new(client), &tool_info)?;
/// ```
pub struct McpTool {
    name: String,
    description: String,
    parameters: serde_json::Value,
    client: Arc<McpClient>,
    server_tool_name: String,
}

impl McpTool {
    /// Create an `McpTool` from a shared `McpClient` and a `ToolInfo`.
    ///
    /// The resulting tool delegates `execute()` to `client.call()`.
    pub fn new(
        client: Arc<McpClient>,
        tool_info: &ToolInfo,
    ) -> Self {
        Self {
            name: tool_info.name.clone(),
            description: tool_info.description.clone(),
            parameters: tool_info.input_schema.clone(),
            client,
            server_tool_name: tool_info.name.clone(),
        }
    }
}

impl std::fmt::Debug for McpTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpTool")
            .field("name", &self.name)
            .field("server_tool_name", &self.server_tool_name)
            .finish()
    }
}

#[async_trait]
impl ITool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        self.parameters.clone()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        // Convert JSON Value arguments to HashMap<String, Value> for MCP call
        let args_map: HashMap<String, serde_json::Value> = match arguments {
            serde_json::Value::Object(map) => map.into_iter().collect(),
            serde_json::Value::Null => HashMap::new(),
            other => {
                // Single-arg tools: wrap in a map if possible, or pass as-is
                let mut map = HashMap::new();
                map.insert("value".to_string(), other);
                map
            }
        };

        let result = self
            .client
            .call(&self.server_tool_name, args_map)
            .await
            .map_err(|e| rust_agent_core::AgentError::ToolError(format!(
                "MCP tool '{}' call failed: {}",
                self.server_tool_name, e
            )))?;

        if result.is_error {
            let error_text = crate::types::ToolContent::extract_text(&result.content);
            return Ok(ToolResult::error(format!(
                "MCP tool '{}' returned error: {}",
                self.server_tool_name,
                if error_text.is_empty() { "unknown error" } else { &error_text }
            )));
        }

        // Extract text content; if there's a single text block, return it as
        // the data payload. Otherwise, return the full content array.
        let text = crate::types::ToolContent::extract_text(&result.content);
        Ok(ToolResult::success(serde_json::json!({
            "text": text,
            "raw_content": result.content,
        })))
    }
}

// ── McpServerClient ────────────────────────────────────────────────────────

/// Manages an MCP server connection and provides tool discovery.
///
/// `McpServerClient` owns the `McpClient` connection and provides a
/// `discover_tools()` method that returns `McpTool` instances ready to
/// be registered with a `ToolRegistry` or `AgentBuilder`.
///
/// # Example
///
/// ```ignore
/// let config = McpConnectionOptions::stdio("filesystem-server", vec!["/tmp"]);
/// let server = McpServerClient::connect(config).await?;
/// let tools = server.discover_tools().await?;
///
/// for tool in tools {
///     builder = builder.with_tool(tool);
/// }
/// ```
pub struct McpServerClient {
    client: Arc<McpClient>,
    connection_config: McpConnectionOptions,
}

impl McpServerClient {
    /// Connect to an MCP server using the given configuration.
    ///
    /// Performs the full MCP initialize handshake.
    pub async fn connect(config: McpConnectionOptions) -> Result<Self, crate::client::McpError> {
        let client = McpClient::connect(config.clone()).await?;
        Ok(Self {
            client: Arc::new(client),
            connection_config: config,
        })
    }

    /// Get a reference to the underlying `McpClient`.
    pub fn client(&self) -> &Arc<McpClient> {
        &self.client
    }

    /// Get the server info from the initialize handshake.
    pub fn server_name(&self) -> Option<&str> {
        self.client.server_info().map(|i| i.name.as_str())
    }

    /// Discover all tools exposed by this MCP server.
    ///
    /// Returns a list of `McpTool` instances, each wrapping a shared
    /// reference to the underlying `McpClient` connection.
    pub async fn discover_tools(&self) -> Result<Vec<McpTool>, crate::client::McpError> {
        let tools_result = match self.client.list_tools(None).await? {
            Some(tools) => tools,
            None => return Ok(Vec::new()),
        };

        let tools: Vec<McpTool> = tools_result
            .tools
            .iter()
            .map(|tool_info| McpTool::new(Arc::clone(&self.client), tool_info))
            .collect();

        tracing::info!(
            server = %self.client.server_info().map(|i| &*i.name).unwrap_or("unknown"),
            count = tools.len(),
            "Discovered MCP tools"
        );

        Ok(tools)
    }

    /// Get the connection configuration (for serialization/resumption).
    pub fn connection_config(&self) -> &McpConnectionOptions {
        &self.connection_config
    }

    /// Internal constructor from pre-connected client.
    #[doc(hidden)]
    pub fn from_client_inner(client: Arc<McpClient>, config: McpConnectionOptions) -> Self {
        Self {
            client,
            connection_config: config,
        }
    }
}

impl std::fmt::Debug for McpServerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServerClient")
            .field("config", &self.connection_config)
            .field("connected", &self.client.is_initialized())
            .finish()
    }
}

// ── Convenience Functions ──────────────────────────────────────────────────

/// Quickly discover all tools from an MCP server given a connection config.
///
/// Useful for one-shot tool discovery and registration:
///
/// ```ignore
/// let tools = rust_agent_mcp::discover_mcp_tools(
///     McpConnectionOptions::stdio("my-server", vec![]),
/// ).await?;
/// ```
pub async fn discover_mcp_tools(
    config: McpConnectionOptions,
) -> Result<Vec<McpTool>, crate::client::McpError> {
    let server = McpServerClient::connect(config).await?;
    server.discover_tools().await
}

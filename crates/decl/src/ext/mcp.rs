use std::sync::Arc;

use rust_agent_core::{IChatClient, ITool};
use rust_agent_framework::AgentBuilder;
use rust_agent_mcp::{McpConnectionOptions, McpContextProvider, McpServerClient};

use crate::error::Result;

/// Extension trait for `AgentBuilder` to support MCP tool integration.
///
/// Requires the `mcp` Cargo feature. Provides convenience methods to register
/// MCP server tools either as a dynamic `McpContextProvider` or by eagerly
/// connecting and registering individual tools.
#[allow(async_fn_in_trait)]
pub trait AgentBuilderMcpExt<C: IChatClient + 'static>: Sized {
    /// Add an MCP context provider that dynamically discovers and injects
    /// tools from the specified MCP server on each agent invocation.
    fn with_mcp_server_provider(self, provider: McpContextProvider) -> Self;

    /// Connect to an MCP server and eagerly register all discovered tools.
    async fn with_mcp_server(self, config: McpConnectionOptions) -> Result<Self>;

    /// Register a single tool from an MCP server by name.
    async fn with_mcp_tool(self, server: &McpServerClient, tool_name: &str) -> Result<Self>;
}

impl<C: IChatClient + 'static> AgentBuilderMcpExt<C> for AgentBuilder<C> {
    fn with_mcp_server_provider(self, provider: McpContextProvider) -> Self {
        self.add_context_provider(provider)
    }

    async fn with_mcp_server(self, config: McpConnectionOptions) -> Result<Self> {
        let server = McpServerClient::connect(config).await.map_err(|e| {
            crate::error::DeclError::Resolution(format!(
                "Failed to connect to MCP server: {}",
                e
            ))
        })?;
        let _tools = server.discover_tools().await.map_err(|e| {
            crate::error::DeclError::Resolution(format!(
                "Failed to discover MCP tools: {}",
                e
            ))
        })?;
        Ok(self.add_context_provider(McpContextProvider::new_shared(Arc::new(server))))
    }

    async fn with_mcp_tool(self, server: &McpServerClient, tool_name: &str) -> Result<Self> {
        let tools = server.discover_tools().await.map_err(|e| {
            crate::error::DeclError::Resolution(format!(
                "Failed to discover MCP tools: {}",
                e
            ))
        })?;
        let mcp_tool = tools.into_iter().find(|t| t.name() == tool_name).ok_or_else(|| {
            crate::error::DeclError::Missing(format!(
                "MCP server does not expose a tool named '{}'",
                tool_name
            ))
        })?;
        Ok(self.with_tool(super::wrappers::ToolWrapper(Arc::new(mcp_tool))))
    }
}

//! Extension traits and wrapper types for use across the decl crate.

use std::sync::Arc;

use rust_agent_core::{IChatClient, ITool};
use rust_agent_framework::AgentBuilder;
use rust_agent_mcp::{McpConnectionOptions, McpContextProvider, McpServerClient};

use crate::error::Result;

// ── MCP Extension (AgentBuilderMcpExt) ──

/// Extension trait for `AgentBuilder` to support MCP tool integration.
///
/// Provides convenience methods to register MCP server tools either
/// as a `McpContextProvider` (dynamic discovery on each invocation)
/// or by eagerly connecting and registering individual tools.
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
                "Failed to connect to MCP server: {}", e
            ))
        })?;
        let _tools = server.discover_tools().await.map_err(|e| {
            crate::error::DeclError::Resolution(format!(
                "Failed to discover MCP tools: {}", e
            ))
        })?;
        Ok(self.add_context_provider(McpContextProvider::new_shared(Arc::new(server))))
    }

    async fn with_mcp_tool(self, server: &McpServerClient, tool_name: &str) -> Result<Self> {
        let tools = server.discover_tools().await.map_err(|e| {
            crate::error::DeclError::Resolution(format!(
                "Failed to discover MCP tools: {}", e
            ))
        })?;
        let mcp_tool = tools.into_iter().find(|t| t.name() == tool_name).ok_or_else(|| {
            crate::error::DeclError::Missing(format!(
                "MCP server does not expose a tool named '{}'", tool_name
            ))
        })?;
        Ok(self.with_tool(ToolWrapper(Arc::new(mcp_tool))))
    }
}

// ── Wrapper Types ──

/// Wraps `Arc<dyn IChatClient>` to implement `IChatClient`, for use with `AgentBuilder<C>`.
pub struct ChatClientWrapper(pub Arc<dyn IChatClient>);

#[async_trait::async_trait]
impl IChatClient for ChatClientWrapper {
    fn model_id(&self) -> &str {
        self.0.model_id()
    }

    fn model_metadata(&self) -> Option<&rust_agent_core::ModelMetadata> {
        self.0.model_metadata()
    }

    async fn run(
        &self,
        messages: &[rust_agent_core::ChatMessage],
        options: rust_agent_core::ChatClientRunOptions,
    ) -> rust_agent_core::Result<
        rust_agent_core::BoxStream<'static, rust_agent_core::Result<rust_agent_core::AgentResponseUpdate>>,
    > {
        self.0.run(messages, options).await
    }
}

/// Wraps `Arc<dyn ITool>` to implement `ITool`, for use with `AgentBuilder::with_tool()`.
pub struct ToolWrapper(pub Arc<dyn ITool>);

#[async_trait::async_trait]
impl ITool for ToolWrapper {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn parameters(&self) -> serde_json::Value {
        self.0.parameters()
    }

    async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<rust_agent_core::ToolResult> {
        self.0.execute(arguments).await
    }

    fn kind(&self) -> &str {
        self.0.kind()
    }
}

//! MCP Context Provider — dynamically injects MCP tools into agent invocations.
//!
//! Implements `IContextProvider` to discover and inject MCP tools at runtime,
//! enabling agents to use MCP server tools without static registration.

use async_trait::async_trait;
use std::sync::Arc;

use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextResult, IAgent, IContextProvider,
    ISession, Result,
};

use crate::tool_adapter::McpServerClient;

/// Context provider that discovers tools from MCP servers and injects them
/// into the agent's tool set at invocation time.
///
/// # Usage
///
/// ```ignore
/// use rust_agent_mcp::{McpContextProvider, McpServerClient, McpConnectionOptions};
///
/// let config = McpConnectionOptions::stdio("my-mcp-server", vec![]);
/// let server = McpServerClient::connect(config).await?;
///
/// let mut builder = AgentBuilder::new("my-agent")
///     .chat_client(client)
///     .add_context_provider(McpContextProvider::new(server));
/// ```
pub struct McpContextProvider {
    servers: Vec<McpServerEntry>,
    cache: tokio::sync::Mutex<Option<Vec<Arc<dyn rust_agent_core::ITool>>>>,
}

struct McpServerEntry {
    server: Arc<McpServerClient>,
}

impl McpContextProvider {
    /// Create a provider connected to a single MCP server.
    pub fn new(server: McpServerClient) -> Self {
        Self {
            servers: vec![McpServerEntry { server: Arc::new(server) }],
            cache: tokio::sync::Mutex::new(None),
        }
    }

    /// Create a provider with an already-shared MCP server client.
    pub fn new_shared(server: Arc<McpServerClient>) -> Self {
        Self {
            servers: vec![McpServerEntry { server }],
            cache: tokio::sync::Mutex::new(None),
        }
    }

    /// Add another MCP server to this provider (multi-server support).
    pub fn add_server(mut self, server: McpServerClient) -> Self {
        self.servers.push(McpServerEntry { server: Arc::new(server) });
        self
    }

    /// Discover all tools from all registered MCP servers.
    pub async fn discover_all_tools(&self) -> Vec<Arc<dyn rust_agent_core::ITool>> {
        let mut all_tools = Vec::new();
        for entry in &self.servers {
            match entry.server.discover_tools().await {
                Ok(tools) => {
                    let count = tools.len();
                    for tool in tools {
                        all_tools.push(Arc::new(tool) as Arc<dyn rust_agent_core::ITool>);
                    }
                    tracing::info!(
                        server = %entry.server.server_name().unwrap_or("unknown"),
                        tool_count = count,
                        "MCP tools discovered and cached"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        server = %entry.server.server_name().unwrap_or("unknown"),
                        error = %e,
                        "Failed to discover MCP tools"
                    );
                }
            }
        }
        all_tools
    }
}

#[async_trait]
impl IContextProvider for McpContextProvider {
    fn name(&self) -> &str {
        "McpContextProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextResult> {
        // Use cache if available, otherwise discover tools
        let mut cache = self.cache.lock().await;
        let cache_ref: &mut Option<Vec<Arc<dyn rust_agent_core::ITool>>> = &mut cache;
        if cache_ref.is_none() {
            let tools = self.discover_all_tools().await;
            *cache_ref = Some(tools);
        }

        let tools: Vec<Arc<dyn rust_agent_core::ITool>> = cache_ref
            .as_ref()
            .cloned()
            .unwrap_or_default();

        Ok(ContextResult {
            tools,
            ..Default::default()
        })
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        Ok(())
    }
}

impl std::fmt::Debug for McpContextProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpContextProvider")
            .field("server_count", &self.servers.len())
            .finish()
    }
}

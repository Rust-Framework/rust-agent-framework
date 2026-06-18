//! Extension traits for building agents and workflows from declarations.

use std::sync::Arc;

use rust_agent_core::{IChatClient, ITool};
use rust_agent_framework::AgentBuilder;
use rust_agent_mcp::{McpConnectionConfig, McpContextProvider, McpServerClient};
use rust_agent_workflow::builder::WorkflowBuilder;

use crate::document::AgentDocument;
use crate::error::Result;
use crate::resolver::connection_resolver;

// ── AgentBuilder Extension ──

/// `AgentBuilder` 的扩展 trait，支持从 MAF 兼容的 `AgentDocument` 或 `AgentDefinition` 进行声明式构建。
pub trait AgentBuilderExt: Sized {
    /// 从 `AgentDocument` 创建 `AgentBuilder`。
    fn from_doc(doc: &AgentDocument) -> Result<AgentBuilder<ChatClientWrapper>>;

    /// 从 JSON 声明字符串创建 `AgentBuilder`。
    fn from_json_decl(json: &str) -> Result<AgentBuilder<ChatClientWrapper>> {
        let doc = AgentDocument::from_json_str(json)?;
        Self::from_doc(&doc)
    }

    /// 从 YAML 声明字符串创建 `AgentBuilder`。
    #[cfg(feature = "yaml")]
    fn from_yaml_decl(yaml: &str) -> Result<AgentBuilder<ChatClientWrapper>> {
        let doc = AgentDocument::from_yaml_str(yaml)?;
        Self::from_doc(&doc)
    }

    /// 从 TOML 声明字符串创建 `AgentBuilder`。
    #[cfg(feature = "toml")]
    fn from_toml_decl(toml_str: &str) -> Result<AgentBuilder<ChatClientWrapper>> {
        let doc = AgentDocument::from_toml_str(toml_str)?;
        Self::from_doc(&doc)
    }
}

impl AgentBuilderExt for AgentBuilder<ChatClientWrapper> {
    fn from_doc(doc: &AgentDocument) -> Result<AgentBuilder<ChatClientWrapper>> {
        let def = doc.inner_definition();
        let prompt_data = match &def.kind_data {
            crate::definition::AgentKindData::Prompt(data) => data,
            other => {
                return Err(crate::error::DeclError::Unsupported(format!(
                    "AgentBuilderExt only supports prompt agents, got kind: {:?}",
                    std::mem::discriminant(other)
                )));
            }
        };

        let chat_client = connection_resolver::resolve_chat_client(&prompt_data.model)?;

        let mut builder = AgentBuilder::new(&def.name)
            .chat_client(ChatClientWrapper(chat_client))
            .instructions(&prompt_data.instructions)
            .max_tool_rounds(prompt_data.max_tool_rounds);

        if !def.description.is_empty() {
            builder = builder.with_description(&def.description);
        }

        Ok(builder)
    }
}

// ── WorkflowBuilder Extension ──

/// `WorkflowBuilder` 的扩展 trait，支持从 MAF 兼容的 YAML/JSON 进行声明式解析。
pub trait WorkflowBuilderExt: Sized {
    /// 从 JSON 字符串解析工作流文档。
    fn parse_json_decl(json: &str) -> Result<AgentDocument> {
        AgentDocument::from_json_str(json)
    }

    /// 从 YAML 字符串解析工作流文档。
    #[cfg(feature = "yaml")]
    fn parse_yaml_decl(yaml: &str) -> Result<AgentDocument> {
        AgentDocument::from_yaml_str(yaml)
    }

    /// 从 TOML 字符串解析工作流文档。
    #[cfg(feature = "toml")]
    fn parse_toml_decl(toml_str: &str) -> Result<AgentDocument> {
        AgentDocument::from_toml_str(toml_str)
    }
}

impl WorkflowBuilderExt for WorkflowBuilder {}

// ── AgentBuilder MCP Extension ──

/// Extension trait for `AgentBuilder` to support MCP tool integration.
///
/// Provides convenience methods to register MCP server tools either
/// as a `McpContextProvider` (dynamic discovery on each invocation)
/// or by eagerly connecting and registering individual tools.
pub trait AgentBuilderMcpExt<C: IChatClient + 'static>: Sized {
    /// Add an MCP context provider that dynamically discovers and injects
    /// tools from the specified MCP server on each agent invocation.
    ///
    /// Uses `McpContextProvider` under the hood with lazy tool discovery.
    fn with_mcp_server_provider(
        self,
        provider: McpContextProvider,
    ) -> Self;

    /// Connect to an MCP server and eagerly register all discovered tools.
    ///
    /// ```ignore
    /// let builder = AgentBuilder::new("agent")
    ///     .chat_client(client)
    ///     .with_mcp_server(McpConnectionConfig::stdio("filesystem", vec!["/work"]))
    ///     .await?;
    /// ```
    async fn with_mcp_server(
        self,
        config: McpConnectionConfig,
    ) -> Result<Self>;

    /// Register a single tool from an MCP server by name.
    ///
    /// The tool is discovered from the already-connected MCP server.
    async fn with_mcp_tool(
        self,
        server: &McpServerClient,
        tool_name: &str,
    ) -> Result<Self>;
}

impl<C: IChatClient + 'static> AgentBuilderMcpExt<C> for AgentBuilder<C> {
    fn with_mcp_server_provider(
        self,
        provider: McpContextProvider,
    ) -> Self {
        self.add_context_provider(provider)
    }

    async fn with_mcp_server(
        self,
        config: McpConnectionConfig,
    ) -> Result<Self> {
        let server = McpServerClient::connect(config)
            .await
            .map_err(|e| crate::error::DeclError::Resolution(format!(
                "Failed to connect to MCP server: {}", e
            )))?;
        // Trigger eager tool discovery to verify the server is working
        let _tools = server
            .discover_tools()
            .await
            .map_err(|e| crate::error::DeclError::Resolution(format!(
                "Failed to discover MCP tools: {}", e
            )))?;

        Ok(self.add_context_provider(McpContextProvider::new_shared(
            Arc::new(server),
        )))
    }

    async fn with_mcp_tool(
        self,
        server: &McpServerClient,
        tool_name: &str,
    ) -> Result<Self> {
        let tools = server
            .discover_tools()
            .await
            .map_err(|e| crate::error::DeclError::Resolution(format!(
                "Failed to discover MCP tools: {}", e
            )))?;

        let mcp_tool = tools
            .into_iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| crate::error::DeclError::Missing(format!(
                "MCP server does not expose a tool named '{}'",
                tool_name
            )))?;

        Ok(self.with_tool(ToolWrapper(Arc::new(mcp_tool))))
    }
}

// ── Wrapper Types ──

/// 包装 `Arc<dyn IChatClient>` 以实现 `IChatClient`，用于 `AgentBuilder<C>`。
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

/// 包装 `Arc<dyn ITool>` 以实现 `ITool`，用于 `AgentBuilder::with_tool()`。
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
}

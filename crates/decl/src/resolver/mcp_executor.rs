//! MCP request executor for workflow graphs.
//!
//! Implements `IExecutor` to handle `ExecutorKind::McpRequest` nodes,
//! delegating tool execution to an MCP server.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::Result;
use rust_agent_mcp::McpServerClient;
use rust_agent_workflow::engine::IWorkflowContext;
use rust_agent_workflow::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use tokio::sync::mpsc::UnboundedSender;

/// Executor that calls a tool on an MCP server.
///
/// Each `McpRequestExecutor` is bound to a single tool on a single MCP server.
/// Multiple executors can share the same `McpServerClient` connection.
///
/// During execution:
/// 1. Extracts the tool arguments from the workflow context (using state_map)
/// 2. Calls `McpClient::call()`
/// 3. Writes the result to the workflow context state_map
/// 4. Sends progress events for observability
pub struct McpRequestExecutor {
    id: String,
    server: Arc<McpServerClient>,
    tool_name: String,
    arguments: HashMap<String, serde_json::Value>,
    output_variable: Option<String>,
}

impl McpRequestExecutor {
    /// Create a new MCP request executor.
    ///
    /// - `id`: unique executor/node identifier
    /// - `server`: shared MCP server client connection
    /// - `tool_name`: the MCP tool to invoke
    /// - `arguments`: static arguments (from workflow declaration)
    /// - `output_variable`: optional state_map key to write the result to
    pub fn new(
        id: impl Into<String>,
        server: Arc<McpServerClient>,
        tool_name: impl Into<String>,
        arguments: HashMap<String, serde_json::Value>,
        output_variable: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            server,
            tool_name: tool_name.into(),
            arguments,
            output_variable,
        }
    }

    /// Get the underlying MCP server client.
    pub fn server(&self) -> &Arc<McpServerClient> {
        &self.server
    }
}

impl std::fmt::Debug for McpRequestExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpRequestExecutor")
            .field("id", &self.id)
            .field("tool_name", &self.tool_name)
            .field("output_variable", &self.output_variable)
            .finish()
    }
}

#[async_trait]
impl IExecutor for McpRequestExecutor {
    fn id(&self) -> &str {
        &self.id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("mcp_request")]
    }

    async fn handle(
        &self,
        _message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        // Resolve arguments: static values from declaration
        let resolved_args = self.arguments.clone();

        if !resolved_args.is_empty() {
            let _ = progress.send(NodeProgress::Custom {
                key: "mcp".to_string(),
                value: serde_json::json!({
                    "tool": &self.tool_name,
                    "arguments": &resolved_args,
                }),
            });
        }

        // Call the MCP tool
        let result = self
            .server
            .client()
            .call(&self.tool_name, resolved_args.clone())
            .await
            .map_err(|e| {
                rust_agent_core::AgentError::WorkflowError(format!(
                    "MCP tool '{}' failed: {}",
                    self.tool_name, e
                ))
            })?;

        let result_text = rust_agent_mcp::types::ToolContent::extract_text(&result.content);

        // Report result via progress
        let _ = progress.send(NodeProgress::Custom {
            key: "mcp_result".to_string(),
            value: serde_json::json!({
                "tool": &self.tool_name,
                "text": &result_text,
                "is_error": result.is_error,
            }),
        });

        // Write output to workflow state if requested
        if let Some(ref var_name) = self.output_variable {
            ctx.write_state(
                var_name,
                serde_json::json!({
                    "text": &result_text,
                    "content": &result.content,
                    "is_error": result.is_error,
                }),
            )
            .await?;
        }

        if result.is_error {
            return Err(rust_agent_core::AgentError::WorkflowError(format!(
                "MCP tool '{}' returned error: {}",
                self.tool_name,
                if result_text.is_empty() {
                    "unknown error"
                } else {
                    &result_text
                }
            )));
        }

        // Output the result text as a message for downstream nodes
        let messages: Vec<Arc<dyn std::any::Any + Send + Sync>> =
            vec![Arc::new(result_text)];
        Ok(HandlerResult::Messages(messages))
    }
}

// ── Builder ───────────────────────────────────────────────────────────────

/// Configuration for building an `McpRequestExecutor` from an `ExecutorKind::McpRequest`.
#[derive(Debug, Clone)]
pub struct McpExecutorConfig {
    pub server: Arc<McpServerClient>,
    pub server_url: String,
    pub tool_name: String,
    pub arguments: HashMap<String, serde_json::Value>,
    pub output_variable: Option<String>,
}

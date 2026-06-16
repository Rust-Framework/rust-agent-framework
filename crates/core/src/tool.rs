use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::Result;

/// Tool interface following MAF's tool abstraction.
#[async_trait]
pub trait ITool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    async fn execute(&self, arguments: serde_json::Value) -> Result<String>;

    /// Runtime marker: returns `true` if this tool requires human approval before
    /// execution. Default is `false` (execute automatically).
    ///
    /// Only [`ApprovalRequiredTool`] overrides this to return `true`.
    /// Checked by `FunctionInvokingChatClient` before executing tool calls.
    fn requires_approval(&self) -> bool {
        false
    }
}

/// Wraps any [`ITool`], marking it as requiring human approval before execution.
///
/// Corresponds to MAF's `ApprovalRequiredAIFunction`.
///
/// `FunctionInvokingChatClient` checks `requires_approval()` at runtime and,
/// when `true`, emits [`ToolApprovalRequest`](crate::AgentResponseUpdate::ToolApprovalRequest)
/// events instead of executing the tool immediately.
///
/// # Usage
///
/// ```ignore
/// // Agent A: auto-execute
/// builder.with_tool(RunCommand);
///
/// // Agent B: require approval (production)
/// builder.with_tool(ApprovalRequiredTool::new(Arc::new(RunCommand)));
/// ```
#[derive(Clone)]
pub struct ApprovalRequiredTool {
    pub inner: Arc<dyn ITool>,
}

impl ApprovalRequiredTool {
    pub fn new(tool: Arc<dyn ITool>) -> Self {
        Self { inner: tool }
    }
}

impl std::fmt::Debug for ApprovalRequiredTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApprovalRequiredTool")
            .field("inner", &format_args!("{}", self.inner.name()))
            .finish()
    }
}

#[async_trait]
impl ITool for ApprovalRequiredTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn description(&self) -> &str {
        self.inner.description()
    }
    fn parameters(&self) -> serde_json::Value {
        self.inner.parameters()
    }
    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        self.inner.execute(arguments).await
    }
    fn requires_approval(&self) -> bool {
        true
    }
}

/// Caller's response to a [`ToolApprovalRequest`](crate::AgentResponseUpdate::ToolApprovalRequest).
///
/// Corresponds to MAF's `FunctionApprovalResponseContent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolApprovalResponse {
    /// Matches the `call_id` from the corresponding `ToolApprovalRequest`.
    pub call_id: String,
    /// `true` = approve execution, `false` = deny.
    pub approved: bool,
    /// Optional reason for denial (fed back to the LLM).
    pub reason: Option<String>,
}

/// ToolRegistry — manages tool registration and lookup following MAF.
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn ITool>>,
}

impl ToolRegistry {
    pub fn new() -> Self { Self { tools: HashMap::new() } }

    pub fn register(&mut self, tool: impl ITool + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn register_arc(&mut self, tool: Arc<dyn ITool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn ITool>> {
        self.tools.get(name)
    }

    pub fn list(&self) -> Vec<&Arc<dyn ITool>> {
        self.tools.values().collect()
    }

    pub fn len(&self) -> usize { self.tools.len() }
    pub fn is_empty(&self) -> bool { self.tools.is_empty() }
}

impl Default for ToolRegistry {
    fn default() -> Self { Self::new() }
}

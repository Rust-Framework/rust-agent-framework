//! Resolver module — converts MAF-aligned declaration data into runnable
//! agent and workflow instances.
//!
//! ## Module structure
//!
//! - `agent_resolver` — AgentDefinition → Arc\<dyn IAgent\>
//! - `workflow_resolver` — WorkflowAgentData → WorkflowGraph
//! - `tool_resolver` — ToolDecl (7 variants) → Arc\<dyn ITool\>
//! - `connection_resolver` — Connection + Model → IChatClient credentials

pub mod agent_resolver;
pub mod code_sandbox_executor;
pub mod connection_resolver;
#[cfg(feature = "mcp")]
pub mod mcp_executor;
pub mod tool_invoke_executor;
pub mod tool_resolver;
pub mod workflow_resolver;

#[allow(deprecated)]
pub use agent_resolver::{quick_agent, AgentResolver};
pub use code_sandbox_executor::CodeSandboxExecutor;
#[cfg(feature = "mcp")]
pub use mcp_executor::McpRequestExecutor;
pub use tool_invoke_executor::ToolInvokeExecutor;
pub use tool_resolver::{ToolFactoryFn, ToolResolver};
pub use workflow_resolver::{quick_workflow, WorkflowResolver};

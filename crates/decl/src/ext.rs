//! Extension traits that add declarative construction methods to
//! `AgentBuilder` and `WorkflowBuilder`.
//!
//! ```ignore
//! use rust_agent_decl::AgentBuilderExt;
//!
//! // Build an agent directly from a JSON declaration string
//! let agent = AgentBuilder::from_json_decl(json_str)?
//!     .with_tool(my_custom_tool)  // still chainable
//!     .build()?;
//! ```

use std::sync::Arc;

use rust_agent_core::ITool;
use rust_agent_framework::AgentBuilder;
use rust_agent_workflow::builder::WorkflowBuilder;

use crate::agent::AgentDecl;
use crate::error::Result;
use crate::resolver::{ClientWrapper, DefaultAgentResolver};
use crate::workflow::WorkflowDecl;

// ── AgentBuilder Extension ──

/// Extension trait for `AgentBuilder<ClientWrapper>` that adds declarative
/// construction from `AgentDecl`, JSON, YAML, or TOML.
///
/// The returned builder can be further customized with `.with_tool()`,
/// `.add_context_provider()`, etc., before calling `.build()`.
///
/// ```ignore
/// use rust_agent_decl::AgentBuilderExt;
///
/// let json = r#"{"id":"agent","model":{"provider":"openai","model":"gpt-4o","api_key":"sk-xxx"}}"#;
/// let agent = AgentBuilder::from_json_decl(json)?.build()?;
/// ```
pub trait AgentBuilderExt: Sized {
    /// Create an `AgentBuilder` from an `AgentDecl`.
    fn from_decl(decl: &AgentDecl) -> Result<AgentBuilder<ClientWrapper>>;

    /// Create an `AgentBuilder` from a JSON declaration string.
    fn from_json_decl(json: &str) -> Result<AgentBuilder<ClientWrapper>> {
        let decl = AgentDecl::from_json_str(json)?;
        Self::from_decl(&decl)
    }

    /// Create an `AgentBuilder` from a YAML declaration string.
    #[cfg(feature = "yaml")]
    fn from_yaml_decl(yaml: &str) -> Result<AgentBuilder<ClientWrapper>> {
        let decl = AgentDecl::from_yaml_str(yaml)?;
        Self::from_decl(&decl)
    }

    /// Create an `AgentBuilder` from a TOML declaration string.
    #[cfg(feature = "toml")]
    fn from_toml_decl(toml_str: &str) -> Result<AgentBuilder<ClientWrapper>> {
        let decl = AgentDecl::from_toml_str(toml_str)?;
        Self::from_decl(&decl)
    }
}

impl AgentBuilderExt for AgentBuilder<ClientWrapper> {
    fn from_decl(decl: &AgentDecl) -> Result<AgentBuilder<ClientWrapper>> {
        let chat_client = DefaultAgentResolver::build_chat_client(&decl.model)?;

        let mut builder = AgentBuilder::new(&decl.id)
            .chat_client(ClientWrapper(chat_client))
            .instructions(&decl.instructions)
            .max_tool_rounds(decl.max_tool_rounds);

        if !decl.description.is_empty() {
            builder = builder.with_description(&decl.description);
        }

        // Tools: sync-only builtins can be registered here.
        // Rhai tools (async) are skipped — use AgentResolver for full support.
        for tool_ref in &decl.tools {
            if let crate::agent::ToolRef::Builtin { name, .. } = tool_ref {
                if let Ok(tool) = DefaultAgentResolver::resolve_builtin_tool(name) {
                    builder = builder.with_tool(ToolWrapper(tool));
                }
            }
        }

        if !decl.properties.is_empty() {
            let props = decl.properties.clone();
            builder = builder.with_properties(props);
        }

        Ok(builder)
    }
}

// ── WorkflowBuilder Extension ──

/// Extension trait for `WorkflowBuilder` that adds declarative
/// parse helpers from `WorkflowDecl`, JSON, YAML, or TOML.
///
/// Note: Agent node references must be resolved through an `AgentResolver`
/// before calling `.build()`. Use `DefaultWorkflowResolver` for that.
pub trait WorkflowBuilderExt: Sized {
    /// Parse a workflow declaration from a JSON string.
    fn parse_json_decl(json: &str) -> Result<WorkflowDecl> {
        WorkflowDecl::from_json_str(json)
    }

    /// Parse a workflow declaration from a YAML string.
    #[cfg(feature = "yaml")]
    fn parse_yaml_decl(yaml: &str) -> Result<WorkflowDecl> {
        WorkflowDecl::from_yaml_str(yaml)
    }

    /// Parse a workflow declaration from a TOML string.
    #[cfg(feature = "toml")]
    fn parse_toml_decl(toml_str: &str) -> Result<WorkflowDecl> {
        WorkflowDecl::from_toml_str(toml_str)
    }
}

impl WorkflowBuilderExt for WorkflowBuilder {}

// ── Tool Wrapper (re-exported) ──

/// Wraps `Arc<dyn ITool>` so it implements `ITool` for use with
/// `AgentBuilder::with_tool()`.
pub struct ToolWrapper(pub Arc<dyn ITool>);

#[async_trait::async_trait]
impl ITool for ToolWrapper {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.0.parameters_schema()
    }

    async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<String> {
        self.0.execute(arguments).await
    }
}

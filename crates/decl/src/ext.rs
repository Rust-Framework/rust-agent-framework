//! Extension traits for building agents and workflows from declarations.

use std::sync::Arc;

use rust_agent_core::{IChatClient, ITool};
use rust_agent_framework::AgentBuilder;
use rust_agent_workflow::builder::WorkflowBuilder;

use crate::document::AgentDocument;
use crate::error::Result;
use crate::resolver::connection_resolver;

// ── AgentBuilder Extension ──

/// Extension trait for `AgentBuilder` that adds declarative
/// construction from MAF-compatible `AgentDocument` or `AgentDefinition`.
pub trait AgentBuilderExt: Sized {
    /// Create an `AgentBuilder` from an `AgentDocument`.
    fn from_doc(doc: &AgentDocument) -> Result<AgentBuilder<ChatClientWrapper>>;

    /// Create an `AgentBuilder` from a JSON declaration string.
    fn from_json_decl(json: &str) -> Result<AgentBuilder<ChatClientWrapper>> {
        let doc = AgentDocument::from_json_str(json)?;
        Self::from_doc(&doc)
    }

    /// Create an `AgentBuilder` from a YAML declaration string.
    #[cfg(feature = "yaml")]
    fn from_yaml_decl(yaml: &str) -> Result<AgentBuilder<ChatClientWrapper>> {
        let doc = AgentDocument::from_yaml_str(yaml)?;
        Self::from_doc(&doc)
    }

    /// Create an `AgentBuilder` from a TOML declaration string.
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

/// Extension trait for `WorkflowBuilder` that adds declarative
/// parse helpers from MAF-compatible YAML/JSON.
pub trait WorkflowBuilderExt: Sized {
    /// Parse a workflow document from a JSON string.
    fn parse_json_decl(json: &str) -> Result<AgentDocument> {
        AgentDocument::from_json_str(json)
    }

    /// Parse a workflow document from a YAML string.
    #[cfg(feature = "yaml")]
    fn parse_yaml_decl(yaml: &str) -> Result<AgentDocument> {
        AgentDocument::from_yaml_str(yaml)
    }

    /// Parse a workflow document from a TOML string.
    #[cfg(feature = "toml")]
    fn parse_toml_decl(toml_str: &str) -> Result<AgentDocument> {
        AgentDocument::from_toml_str(toml_str)
    }
}

impl WorkflowBuilderExt for WorkflowBuilder {}

// ── Wrapper Types ──

/// Wraps `Arc<dyn IChatClient>` to implement `IChatClient` for use with
/// `AgentBuilder<C>`.
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

/// Wraps `Arc<dyn ITool>` to implement `ITool` for `AgentBuilder::with_tool()`.
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

    async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<String> {
        self.0.execute(arguments).await
    }
}

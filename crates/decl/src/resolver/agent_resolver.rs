use std::sync::Arc;

use rust_agent_core::{IAgent, IChatClient};
use rust_agent_framework::AgentBuilder;

use crate::definition::{AgentDefinition, AgentKindData};
use crate::error::DeclError;
use crate::resolver::connection_resolver;
use crate::resolver::tool_resolver::ToolResolver;

/// Resolver that builds a runnable `IAgent` from an `AgentDefinition`.
///
/// Supports:
/// - `Prompt` agents → built via `AgentBuilder`
/// - `Workflow` agents → delegated to `WorkflowResolver`
/// - `Container` agents → not yet supported (returns error)
pub struct AgentResolver {
    tool_resolver: ToolResolver,
    /// Registry of resolved agents, keyed by name.
    agent_registry: Vec<(String, Arc<dyn IAgent>)>,
}

impl AgentResolver {
    /// Create a new agent resolver with default tool resolver.
    pub fn new() -> Self {
        Self {
            tool_resolver: ToolResolver::new(),
            agent_registry: Vec::new(),
        }
    }

    /// Get mutable access to the tool resolver for factory registration.
    pub fn tool_resolver_mut(&mut self) -> &mut ToolResolver {
        &mut self.tool_resolver
    }

    /// Register a custom tool factory.
    pub fn register_tool_factory(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(std::collections::HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>>
            + Send
            + Sync
            + 'static,
    ) {
        self.tool_resolver.register_factory(name, factory);
    }

    /// Look up a previously-resolved agent by name.
    pub fn get_agent(&self, name: &str) -> Option<Arc<dyn IAgent>> {
        self.agent_registry
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, agent)| Arc::clone(agent))
    }

    /// Resolve an `AgentDefinition` into a runnable `IAgent`.
    pub async fn resolve(&mut self, def: &AgentDefinition) -> crate::Result<Arc<dyn IAgent>> {
        match &def.kind_data {
            AgentKindData::Prompt(data) => self.resolve_prompt(def, data).await,
            AgentKindData::Workflow(_data) => Err(DeclError::Unsupported(
                "Workflow agent resolution requires WorkflowResolver. Use resolve_workflow() instead.".into(),
            )),
            AgentKindData::Container(_data) => Err(DeclError::Unsupported(
                "Container agent resolution is not yet supported in Rust".into(),
            )),
        }
    }

    async fn resolve_prompt(
        &mut self,
        def: &AgentDefinition,
        data: &crate::prompt_agent::PromptAgentData,
    ) -> crate::Result<Arc<dyn IAgent>> {
        // Build chat client
        let chat_client = connection_resolver::resolve_chat_client(&data.model)?;

        // Build agent
        let mut builder = AgentBuilder::new(&def.name)
            .chat_client(ChatClientWrapper(chat_client))
            .instructions(&data.instructions)
            .max_tool_rounds(data.max_tool_rounds);

        if !def.description.is_empty() {
            builder = builder.with_description(&def.description);
        }

        // Register tools
        let tools = self.tool_resolver.resolve_all(&data.tools).await?;
        for tool in tools {
            builder = builder.with_tool(ToolWrapper(tool));
        }

        let agent = builder.build()?;

        // Track in registry
        self.agent_registry
            .push((def.name.clone(), Arc::clone(&agent)));

        Ok(agent)
    }
}

impl Default for AgentResolver {
    fn default() -> Self {
        Self::new()
    }
}

// ── Internal Wrappers ──

/// Wraps `Arc<dyn IChatClient>` to implement `IChatClient` for use with
/// `AgentBuilder<C>`.
struct ChatClientWrapper(pub Arc<dyn IChatClient>);

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
struct ToolWrapper(pub Arc<dyn ITool>);

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

use rust_agent_core::ITool;

// ── Convenience Functions ──

/// Quick one-liner: parse an `AgentDocument` from a file and build the agent.
pub async fn quick_agent(path: &str) -> crate::Result<Arc<dyn IAgent>> {
    let doc = crate::document::AgentDocument::from_json_file(path)?;
    let def = doc.inner_definition();
    let mut resolver = AgentResolver::new();
    resolver.resolve(def).await
}

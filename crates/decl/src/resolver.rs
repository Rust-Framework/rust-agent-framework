use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_client::{DeepSeekChatClient, OpenAiChatClient};
use rust_agent_client::ChatClientOptions;
use rust_agent_core::{IAgent, IChatClient, ITool};
use rust_agent_framework::{
    AgentBuilder, AgentSkillsProvider,
    compression::{CompressionPipeline, SlidingWindowStrategy, TokenBudgetStrategy},
    tools::{
        EditFile, FindFiles, InspectFile, ListFiles, MakeDirectory, MoveFile, ReadFile,
        RemovePath, RunCommand, SearchFile, WriteFile,
    },
};
use rust_agent_framework::token_counter::EstimateCounter;
use rust_agent_rhai::{RhaiExecutor, RhaiTool};
use rust_agent_websearch::{WebFetch, WebSearch};
use rust_agent_workflow::graph::WorkflowGraph;
use rust_agent_workflow::executor::{AgentExecutor, IExecutor};
use rust_agent_workflow::executor::base::TypeTag;
use rust_agent_workflow::builder::WorkflowBuilder;

use crate::agent::{
    AgentDecl, CompressionDecl, ContextProviderDecl, ModelConfig, TokenCounterDecl, ToolRef,
};
use crate::error::{DeclError, Result};
use crate::workflow::{EdgeDecl, NodeDecl, WorkflowDecl};

// ── Agent Resolver Trait ──

/// Resolves `AgentDecl` data into runnable `IAgent` instances.
///
/// The resolver owns the mapping from declaration references (tool names,
/// agent IDs) to concrete implementations. Users can customize tool and
/// agent factories through the resolver's registration API.
#[async_trait]
pub trait AgentResolver: Send + Sync {
    /// Build an `IAgent` from an `AgentDecl`.
    async fn resolve(&self, decl: &AgentDecl) -> Result<Arc<dyn IAgent>>;

    /// Resolve a `ToolRef` into a concrete `ITool`.
    async fn resolve_tool(&self, tool_ref: &ToolRef) -> Result<Arc<dyn ITool>>;

    /// Look up a previously-resolved agent by its `AgentDecl.id`.
    fn get_agent(&self, id: &str) -> Option<Arc<dyn IAgent>>;

    /// Load an agent declaration from a file and build it in one step.
    async fn build_from_json_file(&self, path: &str) -> Result<Arc<dyn IAgent>> {
        let decl = AgentDecl::from_json_file(path)?;
        self.resolve(&decl).await
    }

    /// Load an agent declaration from a YAML file and build it in one step.
    #[cfg(feature = "yaml")]
    async fn build_from_yaml_file(&self, path: &str) -> Result<Arc<dyn IAgent>> {
        let decl = AgentDecl::from_yaml_file(path)?;
        self.resolve(&decl).await
    }

    /// Load an agent declaration from a TOML file and build it in one step.
    #[cfg(feature = "toml")]
    async fn build_from_toml_file(&self, path: &str) -> Result<Arc<dyn IAgent>> {
        let decl = AgentDecl::from_toml_file(path)?;
        self.resolve(&decl).await
    }
}

// ── Workflow Resolver Trait ──

/// Resolves `WorkflowDecl` data into a `WorkflowGraph`.
#[async_trait]
pub trait WorkflowResolver: Send + Sync {
    /// Build a `WorkflowGraph` from a `WorkflowDecl`.
    async fn resolve(&self, decl: &WorkflowDecl) -> Result<WorkflowGraph>;

    /// Build a workflow from a JSON file in one step.
    async fn build_from_json_file(&self, path: &str) -> Result<WorkflowGraph> {
        let decl = WorkflowDecl::from_json_file(path)?;
        self.resolve(&decl).await
    }

    /// Build a workflow from a YAML file in one step.
    #[cfg(feature = "yaml")]
    async fn build_from_yaml_file(&self, path: &str) -> Result<WorkflowGraph> {
        let decl = WorkflowDecl::from_yaml_file(path)?;
        self.resolve(&decl).await
    }

    /// Build a workflow from a TOML file in one step.
    #[cfg(feature = "toml")]
    async fn build_from_toml_file(&self, path: &str) -> Result<WorkflowGraph> {
        let decl = WorkflowDecl::from_toml_file(path)?;
        self.resolve(&decl).await
    }
}

// ── Tool Factory ──

/// Type alias for a custom tool factory function.
pub type ToolFactoryFn =
    Box<dyn Fn(HashMap<String, serde_json::Value>) -> Result<Arc<dyn ITool>> + Send + Sync>;

// ── Default Agent Resolver ──

/// Default implementation of `AgentResolver`.
///
/// Pre-registers all built-in framework tools. Supports custom tool factories
/// and tracks resolved agents for workflow node referencing.
pub struct DefaultAgentResolver {
    /// Custom tool factories keyed by name.
    tool_factories: HashMap<String, ToolFactoryFn>,
    /// Registry of resolved agents (populated by `resolve()`).
    agent_registry: Vec<(String, Arc<dyn IAgent>)>,
}

impl DefaultAgentResolver {
    /// Create a new resolver pre-populated with built-in tool factories.
    pub fn new() -> Self {
        Self {
            tool_factories: HashMap::new(),
            agent_registry: Vec::new(),
        }
    }

    /// Register a custom tool factory.
    pub fn register_tool_factory(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(HashMap<String, serde_json::Value>) -> Result<Arc<dyn ITool>>
            + Send
            + Sync
            + 'static,
    ) {
        self.tool_factories
            .insert(name.into(), Box::new(factory));
    }

    /// Build an `IChatClient` from a `ModelConfig`.
    pub fn build_chat_client(model: &ModelConfig) -> Result<Arc<dyn IChatClient>> {
        let api_key = model.resolve_api_key()?;
        let provider = model.provider.to_lowercase();

        let mut options = match provider.as_str() {
            "openai" => ChatClientOptions::openai(&model.model, api_key),
            "deepseek" => ChatClientOptions::deepseek(&model.model, api_key),
            "custom" => ChatClientOptions {
                api_base: model
                    .base_url
                    .clone()
                    .ok_or_else(|| DeclError::Missing("base_url required for custom provider".into()))?,
                api_key,
                model: model.model.clone(),
                ..Default::default()
            },
            other => {
                return Err(DeclError::Unsupported(format!(
                    "Unknown provider '{}'. Supported: openai, deepseek, custom",
                    other
                )));
            }
        };

        if let Some(temp) = model.temperature {
            options.temperature = Some(temp);
        }
        if let Some(mt) = model.max_tokens {
            options.max_tokens = Some(mt);
        }
        for (k, v) in &model.extra_headers {
            options.extra_headers.insert(k.clone(), v.clone());
        }

        match provider.as_str() {
            "openai" => {
                let client = OpenAiChatClient::new(options)?;
                Ok(Arc::new(client))
            }
            "deepseek" => {
                let client = DeepSeekChatClient::new(options)?;
                Ok(Arc::new(client))
            }
            "custom" => {
                let client = OpenAiChatClient::new(options)?;
                Ok(Arc::new(client))
            }
            _ => unreachable!(),
        }
    }

    /// Resolve a built-in tool by name.
    pub fn resolve_builtin_tool(name: &str) -> Result<Arc<dyn ITool>> {
        let tool: Arc<dyn ITool> = match name {
            "read_file" => Arc::new(ReadFile),
            "write_file" => Arc::new(WriteFile),
            "edit_file" => Arc::new(EditFile),
            "list_files" => Arc::new(ListFiles),
            "inspect_file" => Arc::new(InspectFile),
            "make_directory" => Arc::new(MakeDirectory),
            "remove_path" => Arc::new(RemovePath),
            "move_file" => Arc::new(MoveFile),
            "find_files" => Arc::new(FindFiles),
            "search_file" => Arc::new(SearchFile),
            "run_command" => Arc::new(RunCommand),
            "web_search" => Arc::new(WebSearch),
            "web_fetch" => Arc::new(WebFetch),
            other => {
                return Err(DeclError::Unsupported(format!(
                    "Unknown built-in tool '{}'",
                    other
                )));
            }
        };
        Ok(tool)
    }
}

#[async_trait]
impl AgentResolver for DefaultAgentResolver {
    async fn resolve(&self, decl: &AgentDecl) -> Result<Arc<dyn IAgent>> {
        let chat_client = Self::build_chat_client(&decl.model)?;

        let mut builder = AgentBuilder::new(&decl.id)
            .chat_client(ClientWrapper(chat_client))
            .instructions(&decl.instructions)
            .max_tool_rounds(decl.max_tool_rounds);

        if !decl.description.is_empty() {
            builder = builder.with_description(&decl.description);
        }

        // Register tools
        for tool_ref in &decl.tools {
            let tool = self.resolve_tool(tool_ref).await?;
            builder = builder.with_tool(ToolWrapper(tool));
        }

        // Attach context providers
        for cp in &decl.context_providers {
            match cp {
                ContextProviderDecl::InMemoryHistory => {
                    // Already included by default in AgentBuilder
                }
                ContextProviderDecl::Skills { names } => {
                    // Scan configured skill directories and match by name
                    let dirs: Vec<&std::path::Path> = if decl.skill_directories.is_empty() {
                        vec![std::path::Path::new("./skills")]
                    } else {
                        decl.skill_directories.iter().map(|d| std::path::Path::new(d)).collect()
                    };

                    let provider = if names.is_empty() {
                        // Load all skills from all directories
                        AgentSkillsProvider::scan_dirs(&dirs)?
                    } else {
                        // Load only named skills
                        let mut provider = AgentSkillsProvider::new();
                        for dir in &dirs {
                            if let Ok(dir_provider) = AgentSkillsProvider::scan(dir) {
                                for skill_name in names {
                                    // Find skill by name from the scanned directory
                                    if let Some(skill) = dir_provider.skills.iter()
                                        .find(|s| s.metadata.name == *skill_name)
                                    {
                                        provider = provider.with_skill(skill.clone());
                                    }
                                }
                            }
                        }
                        // Warn about skills not found
                        for name in names {
                            let found = provider.skills.iter()
                                .any(|s| s.metadata.name == *name);
                            if !found {
                                tracing::warn!(
                                    "Skill '{}' not found in directories: {:?}",
                                    name, dirs
                                );
                            }
                        }
                        provider
                    };

                    if !provider.skills.is_empty() {
                        builder = builder.add_context_provider(provider);
                    }
                }
            }
        }

        // Compression strategy
        if let Some(comp) = &decl.compression {
            match comp {
                CompressionDecl::SlidingWindow { window_size } => {
                    let strategy = SlidingWindowStrategy::new(window_size.unwrap_or(50));
                    let pipeline = CompressionPipeline::new()
                        .add_strategy(Box::new(strategy));
                    builder = builder.with_compression_strategy(Arc::new(pipeline));
                }
                CompressionDecl::TokenBudget { .. } => {
                    let strategy = TokenBudgetStrategy::new();
                    let pipeline = CompressionPipeline::new()
                        .add_strategy(Box::new(strategy));
                    builder = builder.with_compression_strategy(Arc::new(pipeline));
                }
            }
        }

        // Token counter
        if let Some(tc) = &decl.token_counter {
            match tc {
                TokenCounterDecl::Estimate => {
                    builder = builder.with_token_counter(Arc::new(EstimateCounter::new()));
                }
            }
        }

        // Properties
        if !decl.properties.is_empty() {
            let props = decl.properties.clone();
            builder = builder.with_properties(props);
        }

        let agent = builder.build()?;
        Ok(agent)
    }

    async fn resolve_tool(&self, tool_ref: &ToolRef) -> Result<Arc<dyn ITool>> {
        match tool_ref {
            ToolRef::Builtin { name, .. } => Self::resolve_builtin_tool(name),
            ToolRef::Rhai {
                name,
                description,
                script_path,
                parameters_schema,
            } => {
                let tool = RhaiTool::from_script_file(
                    name,
                    description,
                    parameters_schema.clone(),
                    script_path,
                )
                .map_err(|e| DeclError::Resolution(format!("Rhai tool error: {}", e)))?;
                Ok(Arc::new(tool))
            }
            ToolRef::Custom { name, config } => {
                let factory = self.tool_factories.get(name).ok_or_else(|| {
                    DeclError::Missing(format!(
                        "No factory registered for custom tool '{}'",
                        name
                    ))
                })?;
                factory(config.clone())
            }
        }
    }

    fn get_agent(&self, id: &str) -> Option<Arc<dyn IAgent>> {
        self.agent_registry
            .iter()
            .rev()
            .find(|(agent_id, _)| agent_id == id)
            .map(|(_, agent)| Arc::clone(agent))
    }
}

// ── Default Workflow Resolver ──

/// Default implementation of `WorkflowResolver`.
///
/// Uses a shared `AgentResolver` to resolve agent node references.
pub struct DefaultWorkflowResolver<'a> {
    agent_resolver: &'a dyn AgentResolver,
}

impl<'a> DefaultWorkflowResolver<'a> {
    pub fn new(agent_resolver: &'a dyn AgentResolver) -> Self {
        Self { agent_resolver }
    }
}

#[async_trait]
impl<'a> WorkflowResolver for DefaultWorkflowResolver<'a> {
    async fn resolve(&self, decl: &WorkflowDecl) -> Result<WorkflowGraph> {
        let mut builder = WorkflowBuilder::new();

        for node in &decl.nodes {
            let executor = self.resolve_node_executor(node).await?;
            let nid = node_id(node).to_string();
            let mut b = builder;
            b = b.add_node(nid.clone(), executor);
            if node_is_output(node) {
                b = b.with_output_from(nid);
            }
            builder = b;
        }

        for edge in &decl.edges {
            match edge {
                EdgeDecl::Direct { source, target } => {
                    builder = builder.add_edge(source.as_str(), target.as_str());
                }
                EdgeDecl::FanOut { source, targets } => {
                    let t: Vec<&str> = targets.iter().map(|s| s.as_str()).collect();
                    builder = builder.add_fan_out_edge(source.as_str(), t);
                }
                EdgeDecl::FanIn { sources, target } => {
                    let s: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
                    builder = builder.add_fan_in_edge(s, target.as_str());
                }
            }
        }

        builder = builder.set_start(&decl.start_node_id);

        for output_id in &decl.output_node_ids {
            builder = builder.with_output_from(output_id.as_str());
        }

        for port in &decl.ports {
            let request_port = rust_agent_workflow::graph::port::RequestPort::new(
                &port.id,
                TypeTag::new("json"),
                TypeTag::new("json"),
                "", // Ports in decl don't specify target_node_id
            );
            builder = builder.add_port(request_port);
        }

        builder.build().map_err(Into::into)
    }
}

impl<'a> DefaultWorkflowResolver<'a> {
    async fn resolve_node_executor(&self, node: &NodeDecl) -> Result<Arc<dyn IExecutor>> {
        match node {
            NodeDecl::Agent {
                id,
                agent_ref,
                agent,
                ..
            } => {
                let agent: Arc<dyn IAgent> = if let Some(inline) = agent {
                    self.agent_resolver.resolve(inline).await?
                } else {
                    self.agent_resolver
                        .get_agent(agent_ref)
                        .ok_or_else(|| {
                            DeclError::Missing(format!(
                                "Agent '{}' not found in registry (referenced by node '{}')",
                                agent_ref, id
                            ))
                        })?
                };
                Ok(Arc::new(AgentExecutor::new(id.as_str(), agent)))
            }
            NodeDecl::Function { id, .. } => Err(DeclError::Unsupported(format!(
                "Function node '{}' requires a registered function factory",
                id
            ))),
            NodeDecl::Rhai { id, script_path, .. } => {
                let script = std::fs::read_to_string(script_path).map_err(|e| {
                    DeclError::Resolution(format!(
                        "Failed to read Rhai script '{}': {}",
                        script_path, e
                    ))
                })?;
                let executor = RhaiExecutor::new(id.as_str(), script, "input");
                Ok(Arc::new(executor))
            }
        }
    }
}

fn node_id(node: &NodeDecl) -> &str {
    match node {
        NodeDecl::Agent { id, .. } => id.as_str(),
        NodeDecl::Function { id, .. } => id.as_str(),
        NodeDecl::Rhai { id, .. } => id.as_str(),
    }
}

fn node_is_output(node: &NodeDecl) -> bool {
    match node {
        NodeDecl::Agent { is_output, .. } => *is_output,
        NodeDecl::Function { is_output, .. } => *is_output,
        NodeDecl::Rhai { is_output, .. } => *is_output,
    }
}

// ── Internal Wrappers ──

/// Wraps `Arc<dyn IChatClient>` so it implements `IChatClient` for use with
/// `AgentBuilder<C>`.
///
/// This is the concrete `C` type used when building agents from declarations.
pub struct ClientWrapper(pub Arc<dyn IChatClient>);

#[async_trait]
impl IChatClient for ClientWrapper {
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

/// Wraps `Arc<dyn ITool>` so it implements `ITool` for use with
/// `AgentBuilder::with_tool()`.
struct ToolWrapper(Arc<dyn ITool>);

#[async_trait]
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

// ── Convenience Functions ──

/// Quick one-liner: parse an `AgentDecl` from a file (format auto-detected by extension),
/// resolve it with defaults, and return the built agent.
///
/// Supported extensions: `.json`, `.yaml`/`.yml` (feature `yaml`), `.toml` (feature `toml`).
pub async fn quick_agent(path: &str) -> Result<Arc<dyn IAgent>> {
    let resolver = DefaultAgentResolver::new();
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("json");

    match ext {
        "json" => resolver.build_from_json_file(path).await,
        #[cfg(feature = "yaml")]
        "yaml" | "yml" => resolver.build_from_yaml_file(path).await,
        #[cfg(feature = "toml")]
        "toml" => resolver.build_from_toml_file(path).await,
        other => Err(DeclError::Unsupported(format!(
            "Unknown file extension '{}'. Use .json, .yaml, or .toml",
            other
        ))),
    }
}

/// Quick one-liner: parse a `WorkflowDecl` from a file and build the graph.
pub async fn quick_workflow(path: &str) -> Result<WorkflowGraph> {
    let agent_resolver = DefaultAgentResolver::new();
    let workflow_resolver = DefaultWorkflowResolver::new(&agent_resolver);
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("json");

    match ext {
        "json" => workflow_resolver.build_from_json_file(path).await,
        #[cfg(feature = "yaml")]
        "yaml" | "yml" => workflow_resolver.build_from_yaml_file(path).await,
        #[cfg(feature = "toml")]
        "toml" => workflow_resolver.build_from_toml_file(path).await,
        other => Err(DeclError::Unsupported(format!(
            "Unknown file extension '{}'",
            other
        ))),
    }
}

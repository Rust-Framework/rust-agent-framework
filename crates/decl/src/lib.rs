//! # rust-agent-decl
//!
//! Declarative agent and workflow definitions via JSON, YAML, or TOML.
//!
//! Aligned with **Microsoft Agent Framework (MAF) AgentSchema v1.0** for
//! full format compatibility. MAF YAML files are directly parseable by this crate,
//! and this crate's serialization output can be consumed by MAF clients.
//!
//! ## Quick start
//!
//! ```ignore
//! use rust_agent_decl::AgentDocument;
//!
//! // Parse a MAF-compatible YAML file
//! let yaml = r#"
//! kind: prompt
//! name: my-agent
//! model:
//!   id: gpt-4o
//!   connection:
//!     kind: key
//!     api_key: $OPENAI_API_KEY
//! instructions: You are a helpful assistant.
//! "#;
//!
//! let doc = AgentDocument::from_yaml_str(yaml)?;
//! ```
//!
//! ## Features
//!
//! - `json` (default): serde_json support
//! - `yaml`: serde_yaml support
//! - `toml`: toml support
//! - `rhai`: Rhai embedded expression engine (RAF script/expression system)
//! - `web`: web_search / web_fetch tools (pulls rust-agent-websearch)
//! - `mustache`: Mustache template rendering (optional)

pub mod actions;
pub mod compression_config;
pub mod connection;
pub mod container_agent;
pub mod context_inheritance;
pub mod context_provider_config;
pub mod definition;
pub mod document;
pub mod error;
pub mod expression;
pub mod model;
pub mod prompt_agent;
pub mod schema;
pub mod template;
pub mod tools;
pub mod workflow_decl;

pub mod compiler;
pub mod decl_agent_builder;
pub mod orchestration;
pub mod orchestration_builder;
pub mod orchestration_decl;
pub mod ext;
pub mod resolver;
pub mod sandbox_factory;

// ── Core document types ──
pub use document::{AgentDocument, AgentManifest, ManifestResource};
pub use definition::{AgentDefinition, AgentKindData};

// ── Core schema types ──
pub use schema::{PropertySchema, Property, PropertyType};
pub use model::{Model, ModelOptions, ApiType};
pub use compression_config::{CompressionDecl, TokenCounterDecl};
pub use connection::{Connection, ConnectionKind, ConnectionDetails, AuthenticationMode};
pub use template::{Template, TemplateFormat, TemplateParser};
pub use tools::{ToolDecl, ToolBinding};

// ── Agent variant types ──
pub use prompt_agent::PromptAgentData;
pub use workflow_decl::{WorkflowAgentData, WorkflowTrigger};
pub use container_agent::ContainerAgentData;

// ── Container types ──
pub use container_agent::{
    ProtocolVersionRecord, ContainerResources,
    EnvironmentVariable, CodeConfiguration,
};

// ── Workflow actions ──
pub use actions::{
    ActionDecl, ConditionBranch, AgentRef, SendActivityPayload,
    QuestionPayload, AgentInput, AgentOutput, ToolOutput, HttpBody,
    MessagePayload, ExternalLoop,
};

// ── Compiler ──
pub use compiler::registry::CompileRegistry;
pub use compiler::{compile_workflow, prewarm_workflow_tools};

// ── Resolver ──
#[allow(deprecated)]
pub use resolver::{
    AgentResolver, McpRequestExecutor, ToolInvokeExecutor, ToolResolver, WorkflowResolver,
    quick_agent, quick_workflow, ToolFactoryFn,
};

// ── Orchestration ──
pub use orchestration_decl::{
    parse_orchestration, OrchestrationDecl, OrchestrationMode, PipelinePhaseDecl,
};

// ── DeclAgentBuilder ──
pub use decl_agent_builder::{DeclAgentBuilder, ValidationReport};

// ── Context Provider Config ──
pub use context_provider_config::ContextProviderDecl;

// ── Extension traits ──
pub use ext::{AgentBuilderMcpExt, ChatClientWrapper, ToolWrapper};

// ── Expression engine ──
pub use expression::ExpressionEngine;

// ── Error ──
pub use error::{DeclError, Result};

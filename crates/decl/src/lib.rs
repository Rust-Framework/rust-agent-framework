//! # rust-agent-decl
//!
//! Declarative agent and workflow definitions via JSON, YAML, or TOML.
//!
//! ## Quick start (extension trait style)
//!
//! ```ignore
//! use rust_agent_decl::AgentBuilderExt;
//! use rust_agent_framework::AgentBuilder;
//!
//! // Build an agent directly from a JSON declaration string
//! let json = r#"{"id":"agent","model":{"provider":"openai","model":"gpt-4o","api_key":"sk-xxx"}}"#;
//! let agent = AgentBuilder::from_json_decl(json)?
//!     .with_tool(my_custom_tool)
//!     .build()?;
//! ```
//!
//! ## Quick start (resolver style)
//!
//! ```ignore
//! use rust_agent_decl::{AgentDecl, DefaultAgentResolver};
//! use rust_agent_decl::resolver::AgentResolver;
//!
//! let decl = AgentDecl::from_json_file("agent.json").unwrap();
//! let resolver = DefaultAgentResolver::new();
//! let agent = resolver.resolve(&decl).await.unwrap();
//! ```
//!
//! ## Features
//!
//! - `json` (default): serde_json support
//! - `yaml`: serde_yaml support
//! - `toml`: toml support

pub mod agent;
pub mod error;
pub mod ext;
pub mod resolver;
pub mod workflow;

pub use agent::{
    AgentDecl, CompressionDecl, ContextProviderDecl, ModelConfig, TokenCounterDecl, ToolRef,
};
pub use error::{DeclError, Result};
pub use ext::{AgentBuilderExt, ToolWrapper, WorkflowBuilderExt};
pub use resolver::{
    quick_agent, quick_workflow, AgentResolver, ClientWrapper, DefaultAgentResolver,
    DefaultWorkflowResolver, WorkflowResolver,
};
pub use workflow::{EdgeDecl, NodeDecl, PortDecl, WorkflowDecl};

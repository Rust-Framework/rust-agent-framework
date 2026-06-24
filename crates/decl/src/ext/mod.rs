//! Extension traits and wrapper types for use across the decl crate.
//!
//! Optional integrations (MCP, RAG, Wiki, web tools) live behind Cargo features.
//! Callers enable only the extensions they need to avoid pulling the full dependency graph.

mod context;
#[cfg(feature = "mcp")]
mod mcp;
mod wrappers;

pub use context::{build_provider_from_decl, build_workspace_provider};
#[cfg(feature = "mcp")]
pub use mcp::AgentBuilderMcpExt;
pub use wrappers::{ChatClientWrapper, ToolWrapper};

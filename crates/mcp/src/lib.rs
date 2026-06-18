//! # rust-agent-mcp
//!
//! MCP (Model Context Protocol) client for the rust-agent-framework.
//!
//! Provides protocol-level connectivity to MCP servers via stdio (subprocess)
//! or HTTP SSE transport, with full support for tool, resource, and prompt
//! discovery and execution.
//!
//! ## Quick start
//!
//! ```ignore
//! use rust_agent_mcp::{McpClient, McpConnectionConfig};
//!
//! // Connect to a local MCP server via stdio
//! let config = McpConnectionConfig::stdio("mcp-server", vec![]);
//! let client = McpClient::connect(config).await?;
//!
//! // List tools
//! if let Some(tools) = client.list_tools(None).await? {
//!     for tool in &tools.tools {
//!         println!("Tool: {} — {}", tool.name, tool.description);
//!     }
//! }
//!
//! // Call a tool
//! use std::collections::HashMap;
//! let result = client.call_tool("my_tool", HashMap::new()).await?;
//! println!("Result: {:?}", result.content);
//! ```
//!
//! ## Features
//!
//! - **Stdio transport**: Spawn and communicate with local MCP servers
//! - **SSE transport**: Connect to remote MCP servers over HTTP
//! - **Full protocol coverage**: tools/list, tools/call, resources/list,
//!   resources/read, prompts/list, prompts/get
//! - **Automatic handshake**: initialize/initialized lifecycle management
//! - **Protocol version negotiation**: MCP 2024-11-05

pub mod client;
pub mod context_provider;
pub mod tool_adapter;
pub mod transport;
pub mod types;

pub use client::{McpClient, McpConnectionConfig, McpError};
pub use context_provider::McpContextProvider;
pub use tool_adapter::{McpTool, McpServerClient, discover_mcp_tools};
pub use transport::{Transport, TransportConfig, TransportError, create_transport};
pub use types::*;

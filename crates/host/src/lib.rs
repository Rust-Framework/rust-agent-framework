//! # Rust Agent Host
//!
//! ACP (Agent Client Protocol) server host for the Rust Agent Framework.
//!
//! Bridges the RAF agent framework to ACP-compatible clients (e.g. GPUI-based AI products)
//! via JSON-RPC 2.0 over Stdio or WebSocket transport.
//!
//! ## Architecture
//!
//! ```text
//! GPUI Client ──(ACP/JSON-RPC 2.0)──► AcpAgentHandler ──► RAF Agents
//!                                   (agent-client-protocol)  (rust-agent-framework)
//! ```
//!
//! ## Key Features
//!
//! - **Multi-agent hosting**: Register multiple agents (built-in + declarative), discoverable by clients
//! - **Tagged streaming**: Each `session/update` carries `_meta.raf.agent_id` so clients can render multi-agent views
//! - **Sub-agent discovery**: `get_subagent()`-based tree traversal exposed via `_raf/subagent_list` / `_raf/subagent_tree`
//! - **Independent sub-agent sessions**: Clients create per-sub-agent sessions for parallel streaming views
//! - **Dual transport**: Stdio (local subprocess) or WebSocket (remote deployment)

pub mod config;
pub mod handler;
pub mod registry;
pub mod bridge;
pub mod agents;
pub mod transport;

// Re-export key types for binary use
pub use config::{HostConfig, load_config};
pub use registry::agent_registry::AgentRegistry;
pub use bridge::session::SessionBridge;
pub use bridge::tracker::SubAgentStatusTracker;
pub use handler::acp_agent::RafAgentHost;
pub use handler::workflow_prompt::WorkflowGraphRegistry;

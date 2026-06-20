//! Stdio transport — standard ACP mode via stdin/stdout.
//!
//! Uses the `agent-client-protocol::Stdio` transport to communicate with
//! the client process over standard I/O. This is the primary ACP transport mode
//! for local subprocess agents.

use std::sync::Arc;
use anyhow::Result;
use tracing::info;

use crate::registry::agent_registry::AgentRegistry;
use crate::bridge::session::SessionBridge;
use crate::handler::acp_agent::RafAgentHost;
use crate::handler::workflow_prompt::WorkflowGraphRegistry;

/// Run the ACP server in Stdio mode.
pub async fn run_stdio(
    registry: Arc<AgentRegistry>,
    session_bridge: Arc<SessionBridge>,
    graph_registry: Arc<tokio::sync::Mutex<WorkflowGraphRegistry>>,
) -> Result<()> {
    info!("Starting ACP server in Stdio mode");

    let host = RafAgentHost {
        registry,
        session_bridge,
        graph_registry,
    };

    // Use the ACP SDK's built-in Stdio transport
    let transport = agent_client_protocol::Stdio::new();

    host.run(transport).await?;

    Ok(())
}

//! Rust Agent Host — ACP server binary entry point.
//!
//! Starts the ACP server in either Stdio (local subprocess) or WebSocket (remote) mode,
//! hosting multiple RAF agent systems for ACP-compatible clients.
//!
//! ## Usage
//!
//! ```bash
//! # Stdio mode (standard ACP, spawn as subprocess)
//! cargo run -p rust-agent-host -- --mode stdio
//!
//! # WebSocket mode (remote deployment)
//! cargo run -p rust-agent-host -- --mode ws --bind 127.0.0.1:9876
//!
//! # With config file and agent declarations
//! cargo run -p rust-agent-host -- --mode ws --config host.toml --agents-dir ./agents
//! ```

use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};

use rust_agent_host::{
    config::{load_config, TransportMode},
    registry::agent_registry::AgentRegistry,
    bridge::session::SessionBridge,
    agents::factory::AgentFactory,
    agents::loader::DeclLoader,
    handler::workflow_prompt::WorkflowGraphRegistry,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rust_agent_host=debug".parse()?)
        )
        .init();

    // Load configuration
    let config = load_config()?;
    info!(?config.mode, ?config.ws_bind, "Configuration loaded");

    // Create agent registry
    let mut registry = AgentRegistry::new();

    // Create workflow graph registry (for HITL-capable agents)
    let mut graph_registry = WorkflowGraphRegistry::new();

    // Register built-in agents
    let factory = AgentFactory::new(&config);
    let builtin_agents = factory.create_all().await?;
    for agent in builtin_agents {
        info!(agent_id = %agent.id(), agent_type = %agent.metadata().agent_type, "Registered built-in agent");
        registry.register(agent);
    }

    // Register the 6-stage dev pipeline agent (rust-agent-coding integration)
    if config.dev_pipeline.enabled {
        match factory.create_dev_pipeline_agent() {
            Ok((agent, graph)) => {
                let agent_id = config.dev_pipeline.agent_id.clone();
                info!(
                    agent_id = %agent_id,
                    "Registered dev-pipeline workflow agent (HITL-capable)"
                );
                registry.register(agent);
                graph_registry.register(agent_id, graph);
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Failed to create dev-pipeline agent, skipping (set --no-dev-pipeline to silence)"
                );
            }
        }
    }

    // Load declarative agents from agents_dir
    if let Some(ref agents_dir) = config.agents_dir {
        let loader = DeclLoader::new(agents_dir, &config);
        let declared_agents = loader.load_all().await?;
        for agent in declared_agents {
            info!(agent_id = %agent.id(), agent_type = %agent.metadata().agent_type, "Registered declared agent");
            registry.register(agent);
        }
    }

    info!(
        agent_count = registry.len(),
        workflow_count = graph_registry.len(),
        "All agents registered"
    );

    // Create session bridge (with optional persistence)
    let session_bridge = if let Some(ref dir) = config.session_store_dir {
        info!(session_store_dir = %dir, "Session persistence enabled");
        let store = rust_agent_framework::FileSystemSessionStore::new(dir);
        Arc::new(SessionBridge::with_store(Arc::new(store)))
    } else {
        Arc::new(SessionBridge::new())
    };
    let graph_registry = Arc::new(tokio::sync::Mutex::new(graph_registry));

    info!("Starting ACP server in {:?} mode", config.mode);

    match config.mode {
        TransportMode::Stdio => {
            rust_agent_host::transport::stdio::run_stdio(
                Arc::new(registry),
                session_bridge,
                graph_registry,
            ).await?;
        }
        TransportMode::Ws => {
            rust_agent_host::transport::websocket::run_ws_server(
                config.ws_bind.clone(),
                Arc::new(registry),
                session_bridge,
                graph_registry,
            ).await?;
        }
    }

    info!("ACP server shut down gracefully");
    Ok(())
}

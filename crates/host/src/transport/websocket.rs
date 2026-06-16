//! WebSocket transport — remote ACP mode via axum WebSocket.
//!
//! Sets up an axum HTTP server with a WebSocket upgrade endpoint (`/acp`),
//! bridging each WebSocket connection to the ACP SDK's byte-stream transport.

use std::sync::Arc;
use anyhow::Result;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{info, warn, error};

use axum::{
    Router,
    extract::ws::{WebSocket, Message, WebSocketUpgrade},
    response::IntoResponse,
    routing::any,
};

use crate::registry::agent_registry::AgentRegistry;
use crate::bridge::session::SessionBridge;
use crate::handler::acp_agent::RafAgentHost;

/// Run the ACP server in WebSocket mode, listening on the given address.
pub async fn run_ws_server(
    bind_addr: String,
    registry: Arc<AgentRegistry>,
    session_bridge: Arc<SessionBridge>,
) -> Result<()> {
    info!(addr = %bind_addr, "Starting ACP WebSocket server");

    let app = Router::new()
        .route("/acp", any(ws_handler))
        .with_state(WsState {
            registry,
            session_bridge,
        });

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    info!("WebSocket server listening on {}", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Shared state for the WebSocket server.
#[derive(Clone)]
struct WsState {
    registry: Arc<AgentRegistry>,
    session_bridge: Arc<SessionBridge>,
}

/// WebSocket upgrade handler.
async fn ws_handler(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<WsState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection.
async fn handle_socket(socket: WebSocket, state: WsState) {
    let addr = "ws-client";

    info!(client = addr, "WebSocket connection established");

    // Split the WebSocket into sender and receiver
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Create a duplex channel pair for ACP byte-stream transport
    let (dup_a, mut dup_b) = tokio::io::duplex(64 * 1024);

    // Wrap with futures compat
    let (reader, writer) = tokio::io::split(dup_a);
    let compat_reader = reader.compat();
    let compat_writer = writer.compat_write();

    // Create the ACP host for this connection
    let host = RafAgentHost {
        registry: state.registry.clone(),
        session_bridge: state.session_bridge.clone(),
    };

    // Spawn the ACP agent
    let acp_handle = tokio::spawn(async move {
        let transport = agent_client_protocol::ByteStreams::new(compat_writer, compat_reader);
        if let Err(e) = host.run(transport).await {
            error!(error = %e, "ACP agent error");
        }
    });

    // Bridge: WebSocket → dup_b (client → ACP)
    let ws_to_acp = tokio::spawn(async move {
        while let Some(msg_result) = ws_receiver.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = dup_b.write_all(text.as_bytes()).await {
                        warn!(error = %e, "Write to duplex failed");
                        break;
                    }
                    if let Err(e) = dup_b.write_all(b"\n").await {
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = dup_b.write_all(&data).await {
                        break;
                    }
                    if let Err(e) = dup_b.write_all(b"\n").await {
                        break;
                    }
                }
                Ok(Message::Close(_)) => {
                    info!(client = addr, "WebSocket closed by client");
                    break;
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                Err(e) => {
                    warn!(error = %e, "WebSocket receive error");
                    break;
                }
            }
        }
    });

    tokio::select! {
        res = acp_handle => {
            if let Err(e) = res { error!(error = %e, "ACP task panicked"); }
        }
        res = ws_to_acp => {
            if let Err(e) = res { error!(error = %e, "WS bridge panicked"); }
        }
    }

    info!(client = addr, "WebSocket connection closed");
}

//! WebSocket transport — remote ACP mode via axum WebSocket.
//!
//! Sets up an axum HTTP server with a WebSocket upgrade endpoint (`/acp`),
//! bridging each WebSocket connection to the ACP SDK's byte-stream transport.
//!
//! # 双向桥接
//!
//! WebSocket 连接建立后，需要双向桥接字节流：
//!
//! ```text
//! WS Client ──(Text/Binary)──► dup_b.write ──► dup_a.read ──► ACP SDK (reader)
//! ACP SDK (writer) ──► dup_a.write ──► dup_b.read ──► WS Client
//! ```
//!
//! - `dup_a` 的 reader/writer 由 ACP SDK 持有（通过 `ByteStreams`）
//! - `dup_b` 的 reader/writer 由 WebSocket 桥接任务持有
//! - 两个桥接任务并发运行，任一退出则连接终止

use std::sync::Arc;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::io::AsyncReadExt;
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
use crate::handler::workflow_prompt::WorkflowGraphRegistry;

/// Run the ACP server in WebSocket mode, listening on the given address.
pub async fn run_ws_server(
    bind_addr: String,
    registry: Arc<AgentRegistry>,
    session_bridge: Arc<SessionBridge>,
    graph_registry: Arc<tokio::sync::Mutex<WorkflowGraphRegistry>>,
) -> Result<()> {
    info!(addr = %bind_addr, "Starting ACP WebSocket server");

    let app = Router::new()
        .route("/acp", any(ws_handler))
        .with_state(WsState {
            registry,
            session_bridge,
            graph_registry,
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
    graph_registry: Arc<tokio::sync::Mutex<WorkflowGraphRegistry>>,
}

/// WebSocket upgrade handler.
async fn ws_handler(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<WsState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle an individual WebSocket connection.
///
/// 建立双向字节流桥接：
/// - WS → ACP：WebSocket 接收的消息写入 `dup_b`，ACP 从 `dup_a` 读取
/// - ACP → WS：ACP 写入 `dup_a` 的响应从 `dup_b` 读取，转发到 WebSocket
async fn handle_socket(socket: WebSocket, state: WsState) {
    let addr = "ws-client";

    info!(client = addr, "WebSocket connection established");

    // Split the WebSocket into sender and receiver
    let (ws_sender, mut ws_receiver) = socket.split();

    // Create a duplex channel pair for ACP byte-stream transport.
    // - dup_a: 由 ACP SDK 持有（reader 读客户端请求，writer 写响应/通知）
    // - dup_b: 由 WebSocket 桥接任务持有（writer 收客户端请求，reader 取响应/通知）
    let (dup_a, dup_b) = tokio::io::duplex(64 * 1024);

    // Wrap dup_a with futures compat for ACP SDK
    let (a_reader, a_writer) = tokio::io::split(dup_a);
    let compat_a_reader = a_reader.compat();
    let compat_a_writer = a_writer.compat_write();

    // Split dup_b into reader and writer halves so they can be owned by
    // separate bridge tasks.
    let (mut b_reader, mut b_writer) = tokio::io::split(dup_b);

    // Create the ACP host for this connection
    let host = RafAgentHost {
        registry: state.registry.clone(),
        session_bridge: state.session_bridge.clone(),
        graph_registry: state.graph_registry.clone(),
    };

    // Spawn the ACP agent — 它会从 compat_a_reader 读请求，向 compat_a_writer 写响应
    let acp_handle = tokio::spawn(async move {
        let transport = agent_client_protocol::ByteStreams::new(compat_a_writer, compat_a_reader);
        if let Err(e) = host.run(transport).await {
            error!(error = %e, "ACP agent error");
        }
    });

    // Bridge 1: WebSocket → dup_b.writer (client → ACP)
    // 接收 WS 消息，写入 dup_b，ACP 从 dup_a 读取
    let ws_to_acp = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        while let Some(msg_result) = ws_receiver.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Err(e) = b_writer.write_all(text.as_bytes()).await {
                        warn!(error = %e, "Write to duplex failed (text)");
                        break;
                    }
                    if let Err(_e) = b_writer.write_all(b"\n").await {
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Err(_e) = b_writer.write_all(&data).await {
                        break;
                    }
                    if let Err(_e) = b_writer.write_all(b"\n").await {
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
        // 关闭 b_writer，让 ACP 读到 EOF 从而优雅退出
        b_writer.shutdown().await.ok();
    });

    // Bridge 2: dup_b.reader → WebSocket (ACP → client)
    // 从 dup_b 读取 ACP 写入的响应/通知，转发到 WebSocket
    let acp_to_ws = tokio::spawn(async move {
        let mut ws_sender = ws_sender;
        let mut buf = vec![0u8; 8192];
        loop {
            match b_reader.read(&mut buf).await {
                Ok(0) => {
                    // EOF: ACP 关闭了写端
                    tracing::debug!("ACP → WS stream ended (EOF)");
                    break;
                }
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    if let Err(e) = ws_sender.send(Message::Text(text.into())).await {
                        warn!(error = %e, "WebSocket send failed");
                        break;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Read from duplex failed");
                    break;
                }
            }
        }
    });

    // 任一任务退出即终止连接
    tokio::select! {
        res = acp_handle => {
            if let Err(e) = res { error!(error = %e, "ACP task panicked"); }
        }
        res = ws_to_acp => {
            if let Err(e) = res { error!(error = %e, "WS→ACP bridge panicked"); }
        }
        res = acp_to_ws => {
            if let Err(e) = res { error!(error = %e, "ACP→WS bridge panicked"); }
        }
    }

    info!(client = addr, "WebSocket connection closed");
}

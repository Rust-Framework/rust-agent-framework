//! Transport abstraction for MCP communication.
//!
//! MCP supports two transport mechanisms:
//! - **Stdio**: Subprocess with JSON-RPC over stdin/stdout (most common for local MCP servers)
//! - **SSE**: HTTP Server-Sent Events with POST for client→server messages (remote servers)

use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::types::JsonRpcMessage;

/// Transport trait — abstracts how JSON-RPC messages are sent and received.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a JSON-RPC message to the server.
    async fn send(&self, message: &JsonRpcMessage) -> Result<(), TransportError>;

    /// Receive the next JSON-RPC message from the server.
    /// Returns `None` when the transport is closed.
    async fn recv(&self) -> Result<Option<JsonRpcMessage>, TransportError>;

    /// Close the transport connection.
    async fn close(&self) -> Result<(), TransportError>;
}

/// Transport-related errors.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Transport closed")]
    Closed,

    #[error("Handshake failed: {0}")]
    Handshake(String),

    #[error("Timeout waiting for response")]
    Timeout,
}

// ── Stdio Transport ───────────────────────────────────────────────────────

/// Stdio-based MCP transport using a child process.
///
/// Writes JSON-RPC messages to stdin, reads from stdout. Stderr is forwarded
/// to tracing for diagnostics.
pub struct StdioTransport {
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    stdout: Arc<Mutex<BufReader<tokio::process::ChildStdout>>>,
    child: Arc<Mutex<tokio::process::Child>>,
}

impl StdioTransport {
    /// Spawn a child process as an MCP server and set up stdio transport.
    ///
    /// `command` is the executable, `args` are the arguments.
    /// Uses `tokio::process::Command` to manage the subprocess lifecycle.
    pub async fn spawn(
        command: impl AsRef<std::path::Path>,
        args: &[&str],
    ) -> Result<Self, TransportError> {
        let mut child = tokio::process::Command::new(command.as_ref());
        for arg in args {
            child.arg(arg);
        }
        child
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = child.spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| {
            TransportError::Handshake("Failed to capture child stdin".into())
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            TransportError::Handshake("Failed to capture child stdout".into())
        })?;

        // Forward stderr to tracing
        if let Some(stderr) = child.stderr.take() {
            let reader = BufReader::new(stderr);
            tokio::spawn(async move {
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(target: "mcp_server_stderr", "{}", line);
                }
            });
        }

        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            child: Arc::new(Mutex::new(child)),
        })
    }

    /// Check if the child process has exited.
    pub async fn try_wait(&self) -> Result<Option<std::process::ExitStatus>, std::io::Error> {
        self.child.lock().await.try_wait()
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send(&self, message: &JsonRpcMessage) -> Result<(), TransportError> {
        let mut json = serde_json::to_vec(message)?;
        json.push(b'\n');

        let mut stdin = self.stdin.lock().await;
        stdin.write_all(&json).await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn recv(&self) -> Result<Option<JsonRpcMessage>, TransportError> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();
        match stdout.read_line(&mut line).await {
            Ok(0) => Ok(None), // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return Ok(None);
                }
                let msg: JsonRpcMessage = serde_json::from_str(trimmed)?;
                Ok(Some(msg))
            }
            Err(e) => Err(TransportError::Io(e)),
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        // Close stdin to signal the process
        drop(self.stdin.lock().await);
        // Kill the child process
        self.child.lock().await.kill().await.ok();
        Ok(())
    }
}

// ── SSE Transport ─────────────────────────────────────────────────────────

/// HTTP SSE-based MCP transport for remote MCP servers.
///
/// Client→Server: POST JSON-RPC messages to `post_url`
/// Server→Client: SSE stream from `sse_url`
pub struct SseTransport {
    post_url: String,
    event_rx: Arc<Mutex<tokio::sync::mpsc::UnboundedReceiver<JsonRpcMessage>>>,
}

impl SseTransport {
    /// Create an SSE transport with a pre-established SSE connection.
    ///
    /// `sse_url` — endpoint for SSE stream
    /// `post_url` — endpoint for posting JSON-RPC messages
    pub async fn connect(
        sse_url: impl Into<String>,
        post_url: impl Into<String>,
    ) -> Result<Self, TransportError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sse_url = sse_url.into();
        let post_url = post_url.into();

        // Spawn SSE listener
        {
            let sse_url = sse_url.clone();
            tokio::spawn(async move {
                if let Err(e) = listen_sse(&sse_url, tx).await {
                    tracing::error!(error = %e, "SSE listener error");
                }
            });
        }

        Ok(Self {
            post_url,
            event_rx: Arc::new(Mutex::new(rx)),
        })
    }
}

async fn listen_sse(
    url: &str,
    tx: tokio::sync::mpsc::UnboundedSender<JsonRpcMessage>,
) -> Result<(), TransportError> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("Accept", "text/event-stream")
        .send()
        .await
        .map_err(|e| TransportError::Handshake(format!("SSE connection failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(TransportError::Handshake(format!(
            "SSE endpoint returned {}",
            response.status()
        )));
    }

    let mut event_type = String::new();
    let mut data = String::new();

    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buf = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| TransportError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        buf.extend_from_slice(&chunk);

        // Parse SSE frames
        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line = std::str::from_utf8(&buf[..pos])
                .unwrap_or("")
                .trim()
                .to_string();
            buf.drain(..=pos);

            if line.is_empty() {
                // Empty line = end of event
                if !data.is_empty() {
                    if event_type == "message" || event_type.is_empty() {
                        match serde_json::from_str::<JsonRpcMessage>(&data) {
                            Ok(msg) => {
                                if tx.send(msg).is_err() {
                                    return Ok(()); // Receiver dropped
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, data = %data, "Failed to parse SSE message");
                            }
                        }
                    }
                }
                event_type.clear();
                data.clear();
            } else if let Some(value) = line.strip_prefix("event: ") {
                event_type = value.to_string();
            } else if let Some(value) = line.strip_prefix("data: ") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
        }
    }

    Ok(())
}

#[async_trait]
impl Transport for SseTransport {
    async fn send(&self, message: &JsonRpcMessage) -> Result<(), TransportError> {
        let client = reqwest::Client::new();
        let json = serde_json::to_string(message)?;

        let resp = client
            .post(&self.post_url)
            .header("Content-Type", "application/json")
            .body(json)
            .send()
            .await
            .map_err(|e| TransportError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        if !resp.status().is_success() {
            return Err(TransportError::Handshake(format!(
                "POST failed with {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }

        Ok(())
    }

    async fn recv(&self) -> Result<Option<JsonRpcMessage>, TransportError> {
        let mut rx = self.event_rx.lock().await;
        match rx.recv().await {
            Some(msg) => Ok(Some(msg)),
            None => Ok(None),
        }
    }

    async fn close(&self) -> Result<(), TransportError> {
        // SSE transport close is handled by dropping the receiver
        Ok(())
    }
}

// ── Transport Factory ─────────────────────────────────────────────────────

/// Configuration for creating an MCP transport.
#[derive(Debug, Clone)]
pub enum TransportConfig {
    /// Stdio subprocess transport.
    Stdio {
        command: String,
        args: Vec<String>,
    },
    /// HTTP SSE transport.
    Sse {
        sse_url: String,
        post_url: String,
    },
}

/// Create a transport from configuration.
pub async fn create_transport(config: TransportConfig) -> Result<Box<dyn Transport>, TransportError> {
    match config {
        TransportConfig::Stdio { command, args } => {
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let transport = StdioTransport::spawn(&command, &args_refs).await?;
            Ok(Box::new(transport))
        }
        TransportConfig::Sse { sse_url, post_url } => {
            let transport = SseTransport::connect(sse_url, post_url).await?;
            Ok(Box::new(transport))
        }
    }
}

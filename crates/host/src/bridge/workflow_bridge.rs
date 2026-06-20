//! Workflow ↔ ACP HITL bridge.
//!
//! The core bridge that connects a `WorkflowRuntime` (from `rust-agent-coding` /
//! `rust-agent-workflow`) to ACP `session/update` notifications, including:
//!
//! - **Tagged streaming**: Each `NodeStreaming` event is forwarded as an ACP
//!   `SessionUpdate` with `_meta.raf.agent_id` set to the node ID.
//! - **HITL via `session/request_permission`**: When the workflow halts (e.g.,
//!   the coding pipeline's `p1_confirm` HumanTaskExecutor), the bridge sends an
//!   ACP `RequestPermissionRequest` to the client. The client's response is used
//!   to resume the workflow via `ResumeCommand::InjectMessage`.
//! - **Lifecycle events**: `NodeInvoking` / `NodeCompleted` are surfaced as
//!   status-change signals in `_meta.raf.status`.

use std::sync::Arc;

use futures_util::StreamExt;
use tracing::{debug, info, warn};

use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::schema::{
    ContentBlock, ContentChunk, MessageId, PermissionOption, PermissionOptionId,
    PermissionOptionKind, RequestPermissionRequest, RequestPermissionResponse,
    RequestPermissionOutcome, SelectedPermissionOutcome, SessionId, SessionNotification,
    SessionUpdate, StopReason, TextContent as AcpText, ToolCall, ToolCallId, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields,
};

use rust_agent_core::ChatMessage;
use rust_agent_workflow::{ResumeCommand, WorkflowEvent, WorkflowGraph, WorkflowRuntime};

use crate::bridge::types::{build_raf_meta, node_chunk_to_acp_update_json};

/// Result of running a workflow to completion (or halt).
pub struct WorkflowRunResult {
    pub stop_reason: StopReason,
}

/// Run a workflow graph to completion, bridging all events to ACP notifications.
///
/// This function:
/// 1. Starts a `WorkflowRuntime` with the given graph and initial message.
/// 2. Consumes the event stream, converting each `WorkflowEvent` to ACP
///    `SessionUpdate` notifications (with `_meta.raf.agent_id` tags).
/// 3. On `WorkflowHalted`, sends `session/request_permission` to the client
///    and awaits the response. The user's choice is used to resume the workflow.
/// 4. Returns when the workflow completes or errors.
///
/// # Arguments
/// - `graph`: The workflow graph (e.g., from `build_dev_pipeline`).
/// - `initial_message`: The user's prompt as a `ChatMessage`.
/// - `session_id`: ACP session ID for notifications.
/// - `conn`: ACP client connection (for sending notifications and permission requests).
pub async fn run_workflow_to_completion(
    graph: WorkflowGraph,
    initial_message: ChatMessage,
    session_id: SessionId,
    conn: ConnectionTo<Client>,
) -> WorkflowRunResult {
    let initial: Arc<dyn std::any::Any + Send + Sync> = Arc::new(initial_message);

    let runtime = match WorkflowRuntime::start(graph, initial, None).await {
        Ok(rt) => rt,
        Err(e) => {
            notify_text(&conn, &session_id, &format!("Failed to start workflow: {}", e));
            return WorkflowRunResult {
                stop_reason: StopReason::EndTurn,
            };
        }
    };

    let mut events = match runtime.events().await {
        Some(ev) => ev,
        None => {
            warn!("No event stream from workflow runtime");
            return WorkflowRunResult {
                stop_reason: StopReason::EndTurn,
            };
        }
    };

    let mut last_node_id = String::new();
    let mut msg_id = 0u64;
    let mut stop_reason = StopReason::EndTurn;

    while let Some(event) = events.next().await {
        match &event {
            WorkflowEvent::WorkflowStarted { start_node_id, .. } => {
                debug!(start_node_id, "Workflow started");
                last_node_id = start_node_id.clone();
            }

            WorkflowEvent::NodeInvoking { node_id, node_name, .. } => {
                debug!(node_id, node_name, "Node invoking");
                last_node_id = node_id.clone();
                // Send a status signal: this sub-agent is now executing
                notify_status_signal(&conn, &session_id, node_id, "executing");
            }

            WorkflowEvent::NodeStreaming { node_id, chunk } => {
                // Convert the NodeChunk to an ACP SessionUpdate and send it
                if let Some(update_json) = node_chunk_to_acp_update_json(chunk) {
                    send_workflow_update(&conn, &session_id, node_id, update_json, &mut msg_id);
                }
            }

            WorkflowEvent::NodeCompleted { node_id, .. } => {
                debug!(node_id, "Node completed");
                notify_status_signal(&conn, &session_id, node_id, "completed");
            }

            WorkflowEvent::NodeFailed { node_id, error } => {
                warn!(node_id, error, "Node failed");
                notify_text(&conn, &session_id, &format!("[Node {} failed: {}]", node_id, error));
                notify_status_signal(&conn, &session_id, node_id, "error");
            }

            WorkflowEvent::Custom { key, data } if key == "halt_payload" => {
                // The workflow is about to halt for HITL confirmation.
                // Extract the form data and send it as an agent message so the
                // user can see what they're confirming.
                let form_text = format_halt_payload(data);
                if !form_text.is_empty() {
                    notify_text_meta(
                        &conn,
                        &session_id,
                        &mut msg_id,
                        &form_text,
                        Some(&last_node_id),
                    );
                }
            }

            WorkflowEvent::WorkflowHalted { .. } => {
                info!(session_id = %session_id.0, "Workflow halted for HITL confirmation");

                // Send session/request_permission to the client
                let user_input = request_hitl_confirmation(
                    &conn,
                    &session_id,
                    &last_node_id,
                )
                .await;

                // Resume the workflow with the user's input
                let message: Arc<dyn std::any::Any + Send + Sync> = Arc::new(user_input);
                if let Err(e) = runtime.resume(ResumeCommand::InjectMessage {
                    target_node_id: last_node_id.clone(),
                    message,
                }) {
                    warn!(error = %e, "Failed to resume workflow");
                    notify_text(&conn, &session_id, &format!("Failed to resume: {}", e));
                    break;
                }
            }

            WorkflowEvent::WorkflowResumed { .. } => {
                debug!("Workflow resumed");
            }

            WorkflowEvent::WorkflowCompleted { .. } => {
                info!("Workflow completed");
                stop_reason = StopReason::EndTurn;
                break;
            }

            WorkflowEvent::WorkflowError { error, node_id } => {
                warn!(error, ?node_id, "Workflow error");
                notify_text(&conn, &session_id, &format!("[Workflow error: {}]", error));
                break;
            }

            WorkflowEvent::WorkflowTimeout { .. } => {
                warn!("Workflow timeout");
                notify_text(&conn, &session_id, "[Workflow timed out]");
                break;
            }

            _ => {
                // Ignore other events (SuperStep, AgentResponse, etc.)
            }
        }
    }

    // Wait for the runtime to fully finish
    if let Err(e) = runtime.wait().await {
        warn!(error = %e, "Workflow runtime wait failed");
    }

    WorkflowRunResult { stop_reason }
}

/// Send a `session/request_permission` to the client and await the response.
///
/// Returns the user's input text:
/// - If the user selects "confirm", returns "确认".
/// - If the user selects "revise" with `_meta.raf.feedback`, returns the feedback text.
/// - If the user cancels, returns an empty string.
async fn request_hitl_confirmation(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    node_id: &str,
) -> String {
    // Build a pseudo ToolCallUpdate to carry the permission context.
    // ACP's RequestPermissionRequest requires a `tool_call` field; we use a
    // synthetic tool call ID to represent the HITL confirmation.
    let tool_call_id = ToolCallId::new(format!("hitl_{}", node_id));
    let fields = ToolCallUpdateFields::new()
        .title(format!("人工确认 — 节点 {}", node_id))
        .status(ToolCallStatus::Pending);
    let tool_call = ToolCallUpdate::new(tool_call_id, fields);

    let options = vec![
        PermissionOption::new(
            PermissionOptionId::new("confirm"),
            "确认",
            PermissionOptionKind::AllowOnce,
        ),
        PermissionOption::new(
            PermissionOptionId::new("revise"),
            "提供修改建议",
            PermissionOptionKind::RejectOnce,
        ),
    ];

    let request = RequestPermissionRequest::new(session_id.clone(), tool_call, options)
        .meta({
            let mut m = serde_json::Map::new();
            m.insert("raf.agent_id".into(), serde_json::Value::String(node_id.to_string()));
            m.insert("raf.halt_type".into(), serde_json::Value::String("human_confirmation".to_string()));
            m
        });

    debug!(session_id = %session_id.0, node_id, "Sending HITL permission request");

    match conn.send_request(request).block_task().await {
        Ok(response) => extract_permission_response(&response),
        Err(e) => {
            warn!(error = %e, "HITL permission request failed");
            String::new()
        }
    }
}

/// Extract the user's input from a `RequestPermissionResponse`.
fn extract_permission_response(response: &RequestPermissionResponse) -> String {
    match &response.outcome {
        RequestPermissionOutcome::Cancelled => String::new(),
        RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, meta, .. }) => {
            match option_id.0.as_ref() {
                "confirm" => "确认".to_string(),
                "revise" => {
                    // Try to extract feedback text from _meta.raf.feedback
                    meta.as_ref()
                        .and_then(|m| m.get("raf"))
                        .and_then(|r| r.get("feedback"))
                        .and_then(|f| f.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "请根据反馈修改".to_string())
                }
                other => {
                    // Unknown option; treat as confirmation
                    debug!(option_id = other, "Unknown permission option, treating as confirm");
                    "确认".to_string()
                }
            }
        }
        // `RequestPermissionOutcome` is non-exhaustive; treat any future
        // variants as cancellation (empty input).
        _ => String::new(),
    }
}

/// Format the halt payload (from `HumanTaskExecutor`) into a human-readable string.
fn format_halt_payload(data: &serde_json::Value) -> String {
    let mut parts = Vec::new();

    if let Some(task) = data.get("task").and_then(|v| v.as_str()) {
        parts.push(format!("**任务**: {}", task));
    }
    if let Some(instruction) = data.get("instruction").and_then(|v| v.as_str()) {
        parts.push(format!("**说明**: {}", instruction));
    }
    if let Some(stage) = data.get("stage").and_then(|v| v.as_str()) {
        parts.push(format!("**阶段**: {}", stage));
    }

    if parts.is_empty() {
        // Fallback: just show the raw JSON
        format!("```json\n{}\n```", serde_json::to_string_pretty(data).unwrap_or_default())
    } else {
        parts.join("\n\n")
    }
}

/// Send a workflow streaming update as an ACP notification.
fn send_workflow_update(
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
    node_id: &str,
    update_json: serde_json::Value,
    msg_id: &mut u64,
) {
    // Parse the JSON into a proper SessionUpdate
    let update = parse_session_update(&update_json, msg_id);
    if let Some(update) = update {
        let meta = build_raf_meta(Some(node_id), "executing");
        let notification = SessionNotification::new(session_id.clone(), update).meta(meta);
        let _ = conn.send_notification(notification);
    }
}

/// Parse a JSON value into an ACP `SessionUpdate`.
///
/// This handles the common update types produced by `node_chunk_to_acp_update_json`.
fn parse_session_update(json: &serde_json::Value, msg_id: &mut u64) -> Option<SessionUpdate> {
    let update_type = json.get("sessionUpdate").and_then(|v| v.as_str())?;

    match update_type {
        "agent_message_chunk" => {
            let text = json
                .get("content")
                .and_then(|c| c.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let chunk = ContentChunk::new(ContentBlock::Text(AcpText::new(text)))
                .message_id(MessageId::new(format!("wf_msg_{}", msg_id)));
            *msg_id += 1;
            Some(SessionUpdate::AgentMessageChunk(chunk))
        }
        "tool_call" => {
            let call_id = json.get("toolCallId").and_then(|v| v.as_str())?;
            let title = json.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let tc = ToolCall::new(ToolCallId::new(call_id), title);
            Some(SessionUpdate::ToolCall(tc))
        }
        "tool_call_update" => {
            let call_id = json.get("toolCallId").and_then(|v| v.as_str())?;
            let status_str = json.get("status").and_then(|v| v.as_str()).unwrap_or("in_progress");
            let status = match status_str {
                "completed" => ToolCallStatus::Completed,
                "error" => ToolCallStatus::Failed,
                _ => ToolCallStatus::InProgress,
            };
            let fields = ToolCallUpdateFields::new().status(status);
            let update = ToolCallUpdate::new(ToolCallId::new(call_id), fields);
            Some(SessionUpdate::ToolCallUpdate(update))
        }
        "usage_update" => {
            // UsageUpdate is behind unstable_session_usage feature; skip for now
            None
        }
        _ => None,
    }
}

/// Send a text notification (agent message chunk).
fn notify_text(conn: &ConnectionTo<Client>, sid: &SessionId, text: &str) {
    let chunk = ContentChunk::new(ContentBlock::Text(AcpText::new(text)));
    let _ = conn.send_notification(SessionNotification::new(
        sid.clone(),
        SessionUpdate::AgentMessageChunk(chunk),
    ));
}

/// Send a text notification with agent_id metadata.
fn notify_text_meta(
    conn: &ConnectionTo<Client>,
    sid: &SessionId,
    msg_id: &mut u64,
    text: &str,
    agent_id: Option<&str>,
) {
    let chunk = ContentChunk::new(ContentBlock::Text(AcpText::new(text)))
        .message_id(MessageId::new(format!("wf_msg_{}", msg_id)));
    *msg_id += 1;
    let meta = build_raf_meta(agent_id, "executing");
    let _ = conn.send_notification(
        SessionNotification::new(sid.clone(), SessionUpdate::AgentMessageChunk(chunk)).meta(meta),
    );
}

/// Send a status-change signal (empty agent_message_chunk with status in _meta).
fn notify_status_signal(conn: &ConnectionTo<Client>, sid: &SessionId, agent_id: &str, status: &str) {
    let chunk = ContentChunk::new(ContentBlock::Text(AcpText::new("")));
    let meta = build_raf_meta(Some(agent_id), status);
    let _ = conn.send_notification(
        SessionNotification::new(sid.clone(), SessionUpdate::AgentMessageChunk(chunk)).meta(meta),
    );
}

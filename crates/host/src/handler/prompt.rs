//! Prompt handler — the core streaming bridge from ACP Prompt to RAF IAgent.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use futures_util::StreamExt;
use tracing::{info, debug, warn};

use agent_client_protocol::Client;
use agent_client_protocol::schema::{
    PromptRequest, PromptResponse, StopReason,
    SessionNotification, SessionUpdate, ContentChunk, ContentBlock,
    TextContent as AcpText, MessageId,
    ToolCall, ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
    ToolCallStatus,
};
use agent_client_protocol::ConnectionTo;
use rust_agent_core::{AgentRunOptions, ChatMessage, Content, FinishReason};

use crate::registry::agent_registry::AgentRegistry;
use crate::bridge::session::SessionBridge;
use crate::bridge::model_config::PerTurnModelConfig;
use crate::handler::workflow_prompt::{WorkflowGraphRegistry, handle_workflow_prompt};

/// Route a `session/prompt` request to the appropriate handler based on whether
/// the target agent is a workflow agent (HITL-capable) or a simple agent.
///
/// - If the target agent ID is registered in `WorkflowGraphRegistry`, route to
///   `handle_workflow_prompt` (which uses `WorkflowRuntime` for HITL support).
/// - Otherwise, route to `handle_prompt` (which uses `IAgent::run()` for
///   streaming).
///
/// This is the single entry point used by `RafAgentHost` for `session/prompt`.
pub async fn route_prompt(
    req: PromptRequest,
    responder: agent_client_protocol::Responder<PromptResponse>,
    conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
    bridge: Arc<SessionBridge>,
    graph_registry: Arc<tokio::sync::Mutex<WorkflowGraphRegistry>>,
) -> agent_client_protocol::Result<()> {
    // Determine the target agent ID from request meta (or default).
    let target_agent_id = req.meta.as_ref()
        .and_then(|m| m.get("raf.agent_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // Fall back to the default agent ID from the registry.
            registry.ids().first().map(|id| id.to_string())
        })
        .unwrap_or_else(|| "default".to_string());

    // Check if this is a workflow agent
    let is_workflow = {
        let reg = graph_registry.lock().await;
        reg.contains(&target_agent_id)
    };

    if is_workflow {
        debug!(agent_id = %target_agent_id, "Routing to workflow prompt handler");
        handle_workflow_prompt(req, responder, conn, graph_registry, bridge.clone(), target_agent_id).await
    } else {
        debug!(agent_id = %target_agent_id, "Routing to simple agent prompt handler");
        handle_prompt(req, responder, conn, registry, &bridge).await
    }
}

pub async fn handle_prompt(
    req: PromptRequest,
    responder: agent_client_protocol::Responder<PromptResponse>,
    conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
    bridge: &SessionBridge,
) -> agent_client_protocol::Result<()> {
    let session_id = req.session_id.clone();
    let sid_str = session_id.0.as_ref().to_string();
    debug!(session_id = %sid_str, "Handling prompt request");

    let target_agent_id = req.meta.as_ref()
        .and_then(|m| m.get("raf.agent_id"))
        .and_then(|v| v.as_str());

    // 解析每轮模型配置（从 _meta.raf.model_config）
    let model_config = PerTurnModelConfig::from_meta(req.meta.as_ref());
    if !model_config.is_empty() {
        debug!(session_id = %sid_str, ?model_config, "Per-turn model config applied");
    }

    let agent = match registry.resolve_agent(target_agent_id) {
        Some(a) => a,
        None => {
            warn!(target_agent_id, "Agent not found");
            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
            return Ok(());
        }
    };

    let raf_session = bridge.get_or_create_raf_session(&sid_str).await?;
    let messages = convert_blocks_to_messages(&req.prompt);
    debug!(message_count = messages.len(), "Converted prompt to messages");

    let cancelled = Arc::new(AtomicBool::new(false));
    bridge.register_cancel_token(&sid_str, cancelled.clone()).await;

    // 构建运行选项：取消令牌 + 每轮模型配置覆盖
    // 默认启用思考模式；客户端可通过 model_config.thinking: false 关闭
    let run_opts = {
        let opts = AgentRunOptions::new()
            .with_cancelled(cancelled);
        // 应用每轮模型配置（temperature, max_tokens, thinking, thinking_level）
        // 如果 model_config.thinking 为 None，默认启用思考
        let opts = if model_config.thinking.is_none() {
            opts.with_thinking(true)
        } else {
            opts
        };
        model_config.apply_to_run_options(opts)
    };

    let mut raf_stream = match agent.run(messages, Some(raf_session), Some(run_opts)).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Agent run failed");
            // 通过 session/update 通知客户端错误信息，而非静默返回 EndTurn
            let mut msg_id = 0u64;
            notify_text(&conn, &session_id, &mut msg_id, &format!("[Agent error: {}]", e));
            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
            return Ok(());
        }
    };

    // Background streaming
    tokio::spawn(async move {
        let mut stop_reason = StopReason::EndTurn;
        let mut msg_id = 0u64;
        let sid = session_id;

        while let Some(chunk_result) = raf_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    notify_text(&conn, &sid, &mut msg_id, &format!("Error: {}", e));
                    continue;
                }
            };

            if let Some(ref fr) = chunk.finish_reason {
                stop_reason = match fr {
                    FinishReason::Stop => StopReason::EndTurn,
                    FinishReason::Length => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
            }

            for content in &chunk.contents {
                match content {
                    Content::Text(tc) => {
                        notify_text(&conn, &sid, &mut msg_id, &tc.delta);
                    }
                    Content::Reasoning(rc) => {
                        notify_thought(&conn, &sid, &mut msg_id, &rc.delta);
                    }
                    Content::ToolCallStart(ts) => {
                        notify_tool_start(&conn, &sid, &ts.call_id, &ts.name);
                    }
                    Content::ToolCalled(tc) => {
                        let result_text = tc.result.clone()
                            .or_else(|| tc.error.clone())
                            .unwrap_or_default();
                        notify_tool_done(&conn, &sid, &tc.call_id, tc.error.is_some());
                        if !result_text.is_empty() {
                            notify_text(&conn, &sid, &mut msg_id, &result_text);
                        }
                    }
                    _ => {}
                }
            }
        }

        let _ = responder.respond(PromptResponse::new(stop_reason));
        info!(session_id = %sid.0, ?stop_reason, "Prompt turn completed");
    });

    Ok(())
}

// ── Helper notification functions ──

fn notify_text(conn: &ConnectionTo<Client>, sid: &agent_client_protocol::schema::SessionId,
               msg_id: &mut u64, text: &str) {
    let chunk = ContentChunk::new(ContentBlock::Text(AcpText::new(text)))
        .message_id(MessageId::new(format!("msg_{}", msg_id)));
    *msg_id += 1;
    let _ = conn.send_notification(SessionNotification::new(
        sid.clone(),
        SessionUpdate::AgentMessageChunk(chunk),
    ));
}

fn notify_thought(conn: &ConnectionTo<Client>, sid: &agent_client_protocol::schema::SessionId,
                  msg_id: &mut u64, text: &str) {
    let chunk = ContentChunk::new(ContentBlock::Text(AcpText::new(text)))
        .message_id(MessageId::new(format!("msg_think_{}", msg_id)));
    *msg_id += 1;
    let _ = conn.send_notification(SessionNotification::new(
        sid.clone(),
        SessionUpdate::AgentThoughtChunk(chunk),
    ));
}

fn notify_tool_start(conn: &ConnectionTo<Client>, sid: &agent_client_protocol::schema::SessionId,
                     call_id: &str, name: &str) {
    let tc = ToolCall::new(ToolCallId::new(call_id), name);
    let _ = conn.send_notification(SessionNotification::new(
        sid.clone(),
        SessionUpdate::ToolCall(tc),
    ));
}

fn notify_tool_done(conn: &ConnectionTo<Client>, sid: &agent_client_protocol::schema::SessionId,
                    call_id: &str, is_error: bool) {
    let status = if is_error { ToolCallStatus::Failed } else { ToolCallStatus::Completed };
    let fields = ToolCallUpdateFields::new().status(status);
    let update = ToolCallUpdate::new(ToolCallId::new(call_id), fields);
    let _ = conn.send_notification(SessionNotification::new(
        sid.clone(),
        SessionUpdate::ToolCallUpdate(update),
    ));
}

fn convert_blocks_to_messages(blocks: &[ContentBlock]) -> Vec<ChatMessage> {
    let mut messages = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text(tc) => {
                if !tc.text.is_empty() {
                    messages.push(ChatMessage::user(tc.text.as_str()));
                }
            }
            ContentBlock::ResourceLink(rl) => {
                messages.push(ChatMessage::user(format!("[Reference: {}]", rl.uri)));
            }
            ContentBlock::Resource(er) => {
                use agent_client_protocol::schema::EmbeddedResourceResource;
                if let EmbeddedResourceResource::TextResourceContents(tc) = &er.resource {
                    messages.push(ChatMessage::user(format!("[Resource: {}]\n{}", tc.uri, tc.text)));
                }
            }
            _ => {}
        }
    }
    messages
}

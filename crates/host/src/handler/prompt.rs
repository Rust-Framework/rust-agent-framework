//! Prompt handler — the core streaming bridge from ACP Prompt to RAF IAgent.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use futures_util::StreamExt;
use tracing::{info, debug, warn};

use agent_client_protocol::Client;
use agent_client_protocol::schema::{
    PromptRequest, PromptResponse, StopReason,
    SessionNotification, SessionUpdate, ContentChunk, ContentBlock,
    TextContent as AcpText, MessageId,
    ToolCall, ToolCallId, ToolCallUpdate, ToolCallUpdateFields,
    ToolCallStatus,
    PermissionOption, PermissionOptionId, PermissionOptionKind,
    RequestPermissionRequest, RequestPermissionOutcome, SelectedPermissionOutcome,
};
use agent_client_protocol::ConnectionTo;
use rust_agent_core::{
    AgentRunOptions, ChatMessage, Content, Event, FinishReason, ToolApprovalResponse,
};

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
        handle_prompt(req, responder, conn, registry, bridge.clone()).await
    }
}

pub async fn handle_prompt(
    req: PromptRequest,
    responder: agent_client_protocol::Responder<PromptResponse>,
    conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
    bridge: Arc<SessionBridge>,
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

    // 构建初始运行选项：取消令牌 + 每轮模型配置覆盖
    // 默认启用思考模式；客户端可通过 model_config.thinking: false 关闭
    let initial_opts = {
        let opts = AgentRunOptions::new()
            .with_cancelled(cancelled.clone());
        let opts = if model_config.thinking.is_none() {
            opts.with_thinking(true)
        } else {
            opts
        };
        model_config.apply_to_run_options(opts)
    };

    // Background streaming with approval loop
    let bridge_clone = bridge.clone();
    tokio::spawn(async move {
        let mut stop_reason = StopReason::EndTurn;
        let mut msg_id = 0u64;
        let sid = session_id;

        // 审批循环：run → stream → if AwaitingApproval, request permission → re-run
        let mut current_messages = messages;
        let mut current_opts = initial_opts;
        let mut is_first_run = true;

        loop {
            if cancelled.load(Ordering::SeqCst) {
                stop_reason = StopReason::EndTurn;
                break;
            }

            let mut raf_stream = match agent.run(
                current_messages.clone(),
                Some(raf_session.clone()),
                Some(current_opts.clone()),
            ).await {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "Agent run failed");
                    notify_text(&conn, &sid, &mut msg_id, &format!("[Agent error: {}]", e));
                    break;
                }
            };

            // 首次运行后清空 messages（会话已持有上下文，恢复时无需重复传递）
            if is_first_run {
                current_messages = Vec::new();
                is_first_run = false;
            }

            let mut pending_tool_calls: Vec<(String, String)> = Vec::new();
            let mut approval_needed = false;

            while let Some(chunk_result) = raf_stream.next().await {
                let chunk = match chunk_result {
                    Ok(c) => c,
                    Err(e) => {
                        notify_text(&conn, &sid, &mut msg_id, &format!("Error: {}", e));
                        continue;
                    }
                };

                if let Some(ref fr) = chunk.finish_reason {
                    match fr {
                        FinishReason::Stop => stop_reason = StopReason::EndTurn,
                        FinishReason::Length => stop_reason = StopReason::MaxTokens,
                        FinishReason::AwaitingApproval => {
                            // 工具需要人工审批——标记并继续消费流以收集所有 pending 工具调用
                            approval_needed = true;
                        }
                        FinishReason::MaxRounds => {
                            stop_reason = StopReason::EndTurn;
                            notify_text(&conn, &sid, &mut msg_id,
                                "[Reached max tool rounds — agent stopped]");
                        }
                        FinishReason::ToolCalls => {
                            // 工具调用循环继续——不需要特殊处理
                        }
                        FinishReason::ContentFilter => {
                            stop_reason = StopReason::EndTurn;
                            notify_text(&conn, &sid, &mut msg_id,
                                "[Content filtered by model]");
                        }
                        _ => stop_reason = StopReason::EndTurn,
                    }
                }

                // 处理所有 Content 变体（12 个）
                for content in &chunk.contents {
                    match content {
                        Content::Text(tc) => {
                            notify_text(&conn, &sid, &mut msg_id, &tc.delta);
                        }
                        Content::Reasoning(rc) => {
                            notify_thought(&conn, &sid, &mut msg_id, &rc.delta);
                        }
                        Content::ToolCallStart(ts) => {
                            pending_tool_calls.push((ts.call_id.clone(), ts.name.clone()));
                            notify_tool_start(&conn, &sid, &ts.call_id, &ts.name);
                        }
                        Content::ToolCallArgs(ta) => {
                            // ② 参数流式增量——通知客户端工具调用进行中
                            notify_tool_progress(&conn, &sid, &ta.call_id);
                        }
                        Content::ToolCallEnd(te) => {
                            // ③ 参数接收完毕——通知客户端工具调用进行中
                            notify_tool_progress(&conn, &sid, &te.call_id);
                        }
                        Content::ToolCalling(tc) => {
                            // ④ 完整工具调用（参数已解析）——通知客户端工具调用进行中
                            notify_tool_progress(&conn, &sid, &tc.call_id);
                        }
                        Content::ToolCalled(tc) => {
                            // ⑤ 执行结果——从 pending 列表中移除并通知完成
                            pending_tool_calls.retain(|(id, _)| id != &tc.call_id);
                            notify_tool_done(&conn, &sid, &tc.call_id, tc.error.is_some());
                            let result_text = tc.result.clone()
                                .or_else(|| tc.error.clone())
                                .unwrap_or_default();
                            if !result_text.is_empty() {
                                notify_text(&conn, &sid, &mut msg_id, &result_text);
                            }
                        }
                        Content::Usage(uc) => {
                            // 用量统计——ACP usage_update 在 unstable feature 后，暂记日志
                            debug!(
                                prompt_tokens = uc.usage.prompt_tokens,
                                completion_tokens = uc.usage.completion_tokens,
                                total_tokens = uc.usage.total_tokens,
                                "Usage update"
                            );
                        }
                        Content::Error(ec) => {
                            notify_text(&conn, &sid, &mut msg_id,
                                &format!("[Error {}: {}]", ec.error_code, ec.message));
                        }
                        Content::Uri(uc) => {
                            let label = uc.label.as_deref().unwrap_or("URI");
                            notify_text(&conn, &sid, &mut msg_id,
                                &format!("[{}: {}]", label, uc.uri));
                        }
                        // ToolCallArgsParsed / ToolCallArgsProgress 是细粒度解析事件，无需上报客户端
                        _ => {}
                    }
                }

                // 处理事件（Executor 生命周期 + 自定义事件）
                for event in &chunk.events {
                    match event {
                        Event::ExecutorInvoking(ei) => {
                            debug!(
                                executor_id = %ei.executor_id,
                                executor_type = %ei.executor_type,
                                input_messages = ei.input_message_count,
                                "Executor invoking"
                            );
                        }
                        Event::ExecutorInvoked(ei) => {
                            debug!(
                                executor_id = %ei.executor_id,
                                duration_ms = ei.duration_ms,
                                output_contents = ei.output_content_count,
                                "Executor invoked"
                            );
                        }
                        Event::Custom(ce) => {
                            debug!(event_type = %ce.event_type, "Custom event");
                        }
                    }
                }
            }

            // 审批流程：如果有 pending 工具调用且 finish_reason 为 AwaitingApproval
            if !approval_needed || pending_tool_calls.is_empty() {
                break;
            }

            info!(
                session_id = %sid.0,
                pending_count = pending_tool_calls.len(),
                "Tool approval required — requesting permission from client"
            );

            let mut approval_responses = Vec::new();
            for (call_id, tool_name) in &pending_tool_calls {
                let approved = request_tool_approval(&conn, &sid, call_id, tool_name).await;
                approval_responses.push(ToolApprovalResponse {
                    call_id: call_id.clone(),
                    approved,
                    reason: if approved { None } else { Some("User rejected".to_string()) },
                });
            }

            // 如果所有工具都被拒绝，终止循环
            if approval_responses.iter().all(|r| !r.approved) {
                notify_text(&conn, &sid, &mut msg_id,
                    "[All tool calls rejected by user]");
                break;
            }

            // 准备恢复运行选项：审批响应 + 取消令牌
            current_opts = AgentRunOptions::new()
                .with_cancelled(cancelled.clone())
                .with_tool_approval_responses(approval_responses);
        }

        let _ = responder.respond(PromptResponse::new(stop_reason));
        info!(session_id = %sid.0, ?stop_reason, "Prompt turn completed");

        // 持久化会话（如果配置了会话存储）
        if let Err(e) = bridge_clone.save_session(&sid.0).await {
            warn!(error = %e, "Failed to save session after prompt turn");
        }
        // 清理取消令牌
        bridge_clone.clear_cancel_token(&sid.0).await;
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

/// 通知客户端工具调用正在进行中（参数流式/参数完毕/完整调用阶段）。
fn notify_tool_progress(conn: &ConnectionTo<Client>, sid: &agent_client_protocol::schema::SessionId,
                        call_id: &str) {
    let fields = ToolCallUpdateFields::new().status(ToolCallStatus::InProgress);
    let update = ToolCallUpdate::new(ToolCallId::new(call_id), fields);
    let _ = conn.send_notification(SessionNotification::new(
        sid.clone(),
        SessionUpdate::ToolCallUpdate(update),
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

/// 向客户端发送 `session/request_permission` 请求工具审批，等待用户决策。
///
/// 返回 `true` 表示用户批准执行，`false` 表示拒绝或请求失败。
async fn request_tool_approval(
    conn: &ConnectionTo<Client>,
    session_id: &agent_client_protocol::schema::SessionId,
    call_id: &str,
    tool_name: &str,
) -> bool {
    let tool_call_id = ToolCallId::new(call_id);
    let fields = ToolCallUpdateFields::new()
        .title(format!("工具审批 — {}", tool_name))
        .status(ToolCallStatus::Pending);
    let tool_call = ToolCallUpdate::new(tool_call_id, fields);

    let options = vec![
        PermissionOption::new(
            PermissionOptionId::new("allow"),
            "允许执行",
            PermissionOptionKind::AllowOnce,
        ),
        PermissionOption::new(
            PermissionOptionId::new("deny"),
            "拒绝执行",
            PermissionOptionKind::RejectOnce,
        ),
    ];

    let request = RequestPermissionRequest::new(session_id.clone(), tool_call, options)
        .meta({
            let mut m = serde_json::Map::new();
            m.insert("raf.permission_type".into(),
                serde_json::Value::String("tool_approval".to_string()));
            m.insert("raf.tool_name".into(),
                serde_json::Value::String(tool_name.to_string()));
            m
        });

    debug!(session_id = %session_id.0, call_id, tool_name, "Sending tool approval request");

    match conn.send_request(request).block_task().await {
        Ok(response) => {
            match &response.outcome {
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome { option_id, .. }) => {
                    let approved = option_id.0.as_ref() == "allow";
                    info!(call_id, tool_name, approved, "Tool approval response received");
                    approved
                }
                _ => {
                    warn!(call_id, tool_name, "Tool approval cancelled by client");
                    false
                }
            }
        }
        Err(e) => {
            warn!(error = %e, call_id, tool_name, "Tool approval request failed");
            false
        }
    }
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

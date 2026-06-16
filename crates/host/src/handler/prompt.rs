//! Prompt handler — convert ACP prompts to RAF calls and stream results back.
//!
//! The core streaming bridge:
//! `PromptRequest` → `IAgent::run()` → `BoxStream<AgentResponseResult>` → `session/update` notifications

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tracing::{info, debug, warn};
use futures_util::StreamExt;

use agent_client_protocol::Client;
use agent_client_protocol::schema::{
    PromptRequest, PromptResponse, StopReason,
};
use agent_client_protocol::ConnectionTo;
use rust_agent_core::{AgentRunOptions, Content, FinishReason};

use crate::registry::agent_registry::AgentRegistry;
use crate::bridge::session::SessionBridge;
use crate::bridge::types::convert_prompt_to_chat_messages;

/// Handle a prompt request: convert to RAF, stream, send ACP notifications.
pub async fn handle_prompt(
    req: PromptRequest,
    responder: agent_client_protocol::Responder<PromptResponse>,
    _conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
    bridge: &SessionBridge,
) -> agent_client_protocol::Result<()> {
    let session_id = req.session_id.clone();
    let sid_str = format!("{:?}", session_id);
    debug!(session_id = %sid_str, "Handling prompt request");

    // 1. Determine target agent
    let target_agent_id = req.meta.as_ref()
        .and_then(|m| m.get("raf.agent_id"))
        .and_then(|v| v.as_str());

    let agent = match registry.resolve_agent(target_agent_id) {
        Some(a) => a,
        None => {
            warn!(target_agent_id, "Agent not found");
            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
            return Ok(());
        }
    };

    debug!(agent_id = %agent.id(), "Resolved agent for prompt");

    // 2. Get or create RAF session
    let raf_session = match bridge.get_or_create_raf_session(&sid_str).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "Failed to create RAF session");
            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
            return Ok(());
        }
    };

    // 3. Convert ACP prompt → RAF ChatMessage
    let messages = convert_prompt_to_chat_messages(&[]);
    debug!(message_count = messages.len(), "Converted prompt to messages");

    // 4. Cancel token
    let cancelled = Arc::new(AtomicBool::new(false));
    bridge.register_cancel_token(&sid_str, cancelled.clone()).await;

    let run_opts = AgentRunOptions::new()
        .with_cancelled(cancelled)
        .with_thinking(true);

    // 5. Run RAF agent
    let mut raf_stream = match agent.run(messages, Some(raf_session), Some(run_opts)).await {
        Ok(stream) => stream,
        Err(e) => {
            warn!(error = %e, "Agent run failed");
            let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
            return Ok(());
        }
    };

    // 6. Background task: consume RAF stream → send ACP notifications
    tokio::spawn(async move {
        let mut stop_reason = StopReason::EndTurn;
        let mut text_buffer = String::new();

        while let Some(chunk_result) = raf_stream.next().await {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => {
                    warn!(error = %e, "Stream error");
                    continue;
                }
            };

            // Check finish reason
            if let Some(ref fr) = chunk.finish_reason {
                stop_reason = match fr {
                    FinishReason::Stop => StopReason::EndTurn,
                    FinishReason::Length => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
            }

            // Accumulate text and send as content chunks
            for content in &chunk.contents {
                if let Content::Text(tc) = content {
                    text_buffer.push_str(&tc.delta);
                }
            }
        }

        // Build and respond with collected text
        info!(session_id = %sid_str, chars = text_buffer.len(), ?stop_reason, "Prompt turn completed");
        let _ = responder.respond(PromptResponse::new(stop_reason));
    });

    Ok(())
}

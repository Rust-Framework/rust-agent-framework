//! RAF type → ACP type conversions.
//!
//! Converts RAF `Content`/`Event` and `WorkflowEvent` into ACP `SessionUpdate`
//! variants, with `_meta` tags carrying sub-agent origin information.

use rust_agent_core::Content;
use rust_agent_workflow::{NodeChunk, WorkflowEvent};

/// Convert a RAF `Content` variant into an ACP `SessionUpdate` JSON value.
///
/// Returns `None` for content types that don't map to a session update
/// (they are handled as state tracking signals).
pub fn raf_content_to_acp_update_json(
    content: &Content,
    _agent_id: Option<&str>,
    _status: &str,
) -> Option<serde_json::Value> {

    match content {
        Content::Text(tc) => {
            Some(serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": tc.delta
                }
            }))
        }
        Content::Reasoning(rc) => {
            Some(serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "role": "thought",
                "content": {
                    "type": "text",
                    "text": rc.delta
                }
            }))
        }
        Content::ToolCallStart(ts) => {
            Some(serde_json::json!({
                "sessionUpdate": "tool_call",
                "toolCallId": ts.call_id,
                "title": ts.name,
                "kind": "other",
                "status": "pending"
            }))
        }
        Content::ToolCallArgs(ta) => {
            Some(serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": ta.call_id,
                "status": "in_progress"
            }))
        }
        Content::ToolCallEnd(te) => {
            Some(serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": te.call_id,
                "status": "in_progress"
            }))
        }
        Content::ToolCalling(tc) => {
            Some(serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": tc.call_id,
                "status": "in_progress",
                "argsPreview": tc.arguments
            }))
        }
        Content::ToolCalled(tc) => {
            let status = if tc.error.is_some() { "error" } else { "completed" };
            let content = if let Some(ref result) = tc.result {
                Some(serde_json::json!([{
                    "type": "content",
                    "content": {
                        "type": "text",
                        "text": result
                    }
                }]))
            } else if let Some(ref error) = tc.error {
                Some(serde_json::json!([{
                    "type": "content",
                    "content": {
                        "type": "text",
                        "text": format!("Error: {}", error)
                    }
                }]))
            } else {
                None
            };
            let mut update = serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": tc.call_id,
                "status": status
            });
            if let Some(c) = content {
                update["content"] = c;
            }
            Some(update)
        }
        Content::Usage(uc) => {
            Some(serde_json::json!({
                "sessionUpdate": "usage_update",
                "used": uc.usage.total_tokens,
                "size": 200000  // approximate context window size
            }))
        }
        Content::Error(ec) => {
            Some(serde_json::json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": format!("Error [{}]: {}", ec.error_code, ec.message)
                }
            }))
        }
        // Variants that don't map to updates (parsing progress events)
        Content::ToolCallArgsParsed(_) | Content::ToolCallArgsProgress(_) => None,
        _ => None,
    }
}

/// Extract the source agent ID from a content variant.
pub fn extract_agent_id(content: &Content) -> Option<String> {
    // The RAF framework stores agent_id in ResponseMetadata
    // For ChatClientAgent, it comes from the converter
    match content {
        Content::Text(tc) => tc.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::Reasoning(rc) => rc.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::ToolCallStart(ts) => ts.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::ToolCallArgs(ta) => ta.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::ToolCallEnd(te) => te.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::ToolCalling(tc) => tc.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::ToolCalled(tc) => tc.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::Usage(uc) => uc.meta.agent_id.as_ref().map(|id| id.to_string()),
        Content::Error(ec) => ec.meta.agent_id.as_ref().map(|id| id.to_string()),
        _ => None,
    }
}

/// Build the `_meta` JSON object for a session/update notification.
///
/// Returns a `serde_json::Map<String, Value>` directly, as required by ACP's
/// `SessionNotification::meta()` and `RequestPermissionRequest::meta()` builders.
pub fn build_raf_meta(agent_id: Option<&str>, status: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut meta = serde_json::Map::new();
    if let Some(id) = agent_id {
        meta.insert("raf.agent_id".into(), serde_json::Value::String(id.to_string()));
    }
    meta.insert("raf.status".into(), serde_json::Value::String(status.to_string()));
    meta
}

/// Build the `_meta` JSON object with agent_id, agent_type, and status.
pub fn build_raf_meta_typed(
    agent_id: Option<&str>,
    agent_type: Option<&str>,
    status: &str,
) -> serde_json::Map<String, serde_json::Value> {
    let mut meta = serde_json::Map::new();
    if let Some(id) = agent_id {
        meta.insert("raf.agent_id".into(), serde_json::Value::String(id.to_string()));
    }
    if let Some(t) = agent_type {
        meta.insert("raf.agent_type".into(), serde_json::Value::String(t.to_string()));
    }
    meta.insert("raf.status".into(), serde_json::Value::String(status.to_string()));
    meta
}

/// Convert ACP PromptRequest content blocks to RAF ChatMessages.
pub fn convert_prompt_to_chat_messages(
    prompt: &[serde_json::Value],
) -> Vec<rust_agent_core::ChatMessage> {
    let mut messages = Vec::new();

    for block in prompt {
        let content_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("text");

        match content_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    messages.push(rust_agent_core::ChatMessage::user(text));
                }
            }
            "resource" => {
                if let Some(resource) = block.get("resource") {
                    if let Some(text) = resource.get("text").and_then(|v| v.as_str()) {
                        let uri = resource
                            .get("uri")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(unknown)");
                        let content = format!("[Resource: {}]\n{}", uri, text);
                        messages.push(rust_agent_core::ChatMessage::user(content));
                    }
                }
            }
            "image" => {
                // Images are not yet supported; include a placeholder
                messages.push(rust_agent_core::ChatMessage::user("[Image content - not yet supported]"));
            }
            _ => {
                // Unknown content type; include as text if possible
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    messages.push(rust_agent_core::ChatMessage::user(text));
                }
            }
        }
    }

    messages
}

// ── WorkflowEvent → ACP conversion helpers ──────────────────────────────────

/// Convert a `NodeChunk` (workflow streaming delta) into an ACP `SessionUpdate` JSON value.
///
/// Returns `None` for chunk types that don't produce a user-visible update.
pub fn node_chunk_to_acp_update_json(chunk: &NodeChunk) -> Option<serde_json::Value> {
    match chunk {
        NodeChunk::TextDelta { delta } => Some(serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": delta }
        })),
        NodeChunk::ReasoningDelta { delta } => Some(serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "role": "thought",
            "content": { "type": "text", "text": delta }
        })),
        NodeChunk::ToolCallStart { call_id, name } => Some(serde_json::json!({
            "sessionUpdate": "tool_call",
            "toolCallId": call_id,
            "title": name,
            "kind": "other",
            "status": "pending"
        })),
        NodeChunk::ToolCallArgs { call_id, args_delta } => Some(serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": call_id,
            "status": "in_progress",
            "argsPreview": args_delta
        })),
        NodeChunk::ToolCallEnd { call_id } => Some(serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": call_id,
            "status": "in_progress"
        })),
        NodeChunk::ToolResult { call_id, result } => Some(serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": call_id,
            "status": "completed",
            "content": [{
                "type": "content",
                "content": { "type": "text", "text": result }
            }]
        })),
        NodeChunk::UsageUpdate { prompt_tokens, completion_tokens } => {
            Some(serde_json::json!({
                "sessionUpdate": "usage_update",
                "used": prompt_tokens + completion_tokens,
                "size": 200000
            }))
        }
        NodeChunk::Custom { .. } => None,
    }
}

/// Extract the halt payload from a `WorkflowEvent::Custom { key: "halt_payload", .. }` event.
///
/// Returns `Some(payload)` if the event is a halt payload, `None` otherwise.
pub fn extract_halt_payload(event: &WorkflowEvent) -> Option<&serde_json::Value> {
    match event {
        WorkflowEvent::Custom { key, data } if key == "halt_payload" => Some(data),
        _ => None,
    }
}

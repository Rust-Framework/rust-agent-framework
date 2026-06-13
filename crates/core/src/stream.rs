use std::pin::Pin;

use futures_core::Stream;

use crate::{AgentId, AgentResponse, AgentStreamChunk, Result, ToolCall};

/// Type alias for a boxed, sendable stream.
pub type BoxStream<T> = Pin<Box<dyn Stream<Item = T> + Send>>;

/// Collect an agent stream into a single aggregated AgentResponse.
pub async fn collect_agent_response(
    mut stream: BoxStream<Result<AgentStreamChunk>>,
) -> Result<AgentResponse> {
    let mut text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut source_agent_id: Option<AgentId> = None;

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if let Some(delta) = chunk.text_delta {
            text.push_str(&delta);
        }
        if let Some(tc_delta) = chunk.tool_call_delta {
            // Accumulate tool call deltas into complete tool calls
            let idx = tc_delta.index;
            while tool_calls.len() <= idx {
                tool_calls.push(ToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: serde_json::Value::Null,
                });
            }
            if let Some(id) = tc_delta.id {
                tool_calls[idx].id = id;
            }
            if let Some(name) = tc_delta.name {
                tool_calls[idx].name = name;
            }
            if let Some(args) = tc_delta.arguments_delta {
                if tool_calls[idx].arguments.is_null() {
                    tool_calls[idx].arguments = serde_json::Value::String(args);
                } else if let Some(existing) = tool_calls[idx].arguments.as_str() {
                    tool_calls[idx].arguments = serde_json::Value::String(format!("{}{}", existing, args));
                }
            }
        }
        if chunk.source_agent_id.is_some() {
            source_agent_id = chunk.source_agent_id;
        }
    }

    // After accumulating all deltas, try to parse string arguments into proper JSON
    for tc in &mut tool_calls {
        if let Some(s) = tc.arguments.as_str() {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                tc.arguments = parsed;
            }
        }
    }

    Ok(AgentResponse {
        text,
        tool_calls,
        source_agent_id,
    })
}

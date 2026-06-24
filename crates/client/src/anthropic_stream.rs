//! Anthropic Messages API SSE stream → `AgentResponseUpdate`.

use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use rust_agent_core::{AgentError, AgentResponseUpdate, FinishReason, Result};

use crate::usage::AnthropicUsage;

/// Anthropic SSE: `event: <type>` + `data: <json>` pairs.
pub struct AnthropicSseStream<S> {
    inner: S,
    buffer: Vec<u8>,
    pending: std::vec::IntoIter<Result<AgentResponseUpdate>>,
    done: bool,
    current_event: Option<String>,
    /// Per content block index — tracks in-flight tool_use streams.
    tool_blocks: HashMap<usize, ToolBlockState>,
    response_id: Option<String>,
    response_model: Option<String>,
    accumulated_usage: Option<rust_agent_core::Usage>,
}

#[derive(Debug, Default)]
struct ToolBlockState {
    id: Option<String>,
    name: Option<String>,
}

impl<S> AnthropicSseStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Send + Unpin + 'static,
{
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            pending: Vec::new().into_iter(),
            done: false,
            current_event: None,
            tool_blocks: HashMap::new(),
            response_id: None,
            response_model: None,
            accumulated_usage: None,
        }
    }

    fn map_event(&mut self, event_type: &str, data: &serde_json::Value) -> Vec<Result<AgentResponseUpdate>> {
        let mut events = Vec::new();
        match event_type {
            "message_start" => {
                if let Some(msg) = data.get("message") {
                    self.response_id = msg
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    self.response_model = msg
                        .get("model")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    if self.response_id.is_some() || self.response_model.is_some() {
                        events.push(Ok(AgentResponseUpdate::ResponseMetadata {
                            id: self.response_id.clone(),
                            model: self.response_model.clone(),
                        }));
                    }
                    if let Some(usage_val) = msg.get("usage") {
                        if let Ok(partial) =
                            serde_json::from_value::<AnthropicUsage>(usage_val.clone())
                        {
                            let mut usage = self.accumulated_usage.take().unwrap_or_default();
                            partial.merge_into(&mut usage);
                            usage.merge_raw_usage(usage_val);
                            self.accumulated_usage = Some(usage.clone());
                            events.push(Ok(AgentResponseUpdate::Usage { usage }));
                        }
                    }
                }
            }
            "content_block_start" => {
                if let (Some(index), Some(block)) = (
                    data.get("index").and_then(|v| v.as_u64()),
                    data.get("content_block"),
                ) {
                    let idx = index as usize;
                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        self.tool_blocks.insert(
                            idx,
                            ToolBlockState {
                                id: id.clone(),
                                name: name.clone(),
                            },
                        );
                        if let (Some(id), Some(name)) = (id, name) {
                            events.push(Ok(AgentResponseUpdate::ToolCallStart { id, name }));
                        }
                    }
                }
            }
            "content_block_delta" => {
                if let (Some(index), Some(delta)) = (
                    data.get("index").and_then(|v| v.as_u64()),
                    data.get("delta"),
                ) {
                    let idx = index as usize;
                    match delta.get("type").and_then(|v| v.as_str()) {
                        Some("text_delta") => {
                            if let Some(text) = delta.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    events.push(Ok(AgentResponseUpdate::TextDelta {
                                        delta: text.to_string(),
                                    }));
                                }
                            }
                        }
                        Some("input_json_delta") => {
                            let partial = delta
                                .get("partial_json")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !partial.is_empty() {
                                let state = self.tool_blocks.entry(idx).or_default();
                                events.push(Ok(AgentResponseUpdate::ToolCallDelta {
                                    index: idx,
                                    id: state.id.clone(),
                                    name: state.name.clone(),
                                    arguments_delta: partial.to_string(),
                                }));
                            }
                        }
                        _ => {}
                    }
                }
            }
            "message_delta" => {
                let stop_reason = data
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|v| v.as_str());
                if let Some(usage_val) = data.get("usage") {
                    if let Ok(partial) = serde_json::from_value::<AnthropicUsage>(usage_val.clone())
                    {
                        let mut usage = self.accumulated_usage.take().unwrap_or_default();
                        partial.merge_into(&mut usage);
                        usage.merge_raw_usage(usage_val);
                        self.accumulated_usage = Some(usage);
                    }
                }
                if let Some(reason) = stop_reason {
                    let finish_reason = map_stop_reason(reason);
                    events.push(Ok(AgentResponseUpdate::Finish {
                        finish_reason,
                        usage: self.accumulated_usage.clone(),
                    }));
                }
            }
            "message_stop" => {
                self.done = true;
            }
            "error" => {
                let msg = data
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Anthropic stream error");
                events.push(Err(AgentError::StreamError(msg.to_string())));
            }
            _ => {}
        }
        events
    }
}

fn map_stop_reason(reason: &str) -> FinishReason {
    match reason {
        "end_turn" | "stop_sequence" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "tool_use" => FinishReason::ToolCalls,
        other => FinishReason::Other(other.to_string()),
    }
}

impl<S> Stream for AnthropicSseStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Send + Unpin + 'static,
{
    type Item = Result<AgentResponseUpdate>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(update) = self.pending.next() {
            return Poll::Ready(Some(update));
        }

        loop {
            if self.done && self.pending.next().is_none() {
                return Poll::Ready(None);
            }

            if let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
                let line_bytes = self.buffer.drain(..=pos).collect::<Vec<_>>();
                let line = String::from_utf8_lossy(&line_bytes);
                let trimmed = line.trim();

                if trimmed.is_empty() {
                    continue;
                }

                if let Some(event) = trimmed.strip_prefix("event: ") {
                    self.current_event = Some(event.trim().to_string());
                    continue;
                }

                if let Some(data) = trimmed.strip_prefix("data: ") {
                    let event_type = self.current_event.take().unwrap_or_else(|| "unknown".into());
                    match serde_json::from_str::<serde_json::Value>(data) {
                        Ok(json) => {
                            // Anthropic also embeds type inside data JSON
                            let ty = json
                                .get("type")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&event_type);
                            let mut mapped = self.map_event(ty, &json);
                            if !mapped.is_empty() {
                                let first = mapped.remove(0);
                                self.pending = mapped.into_iter();
                                return Poll::Ready(Some(first));
                            }
                            continue;
                        }
                        Err(e) => {
                            return Poll::Ready(Some(Err(AgentError::StreamError(format!(
                                "Anthropic SSE parse error: {}",
                                e
                            )))));
                        }
                    }
                }
                continue;
            }

            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    self.buffer.extend_from_slice(&bytes);
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    self.done = true;
                    return Poll::Ready(Some(Err(AgentError::StreamError(format!(
                        "HTTP stream error: {}",
                        e
                    )))));
                }
                Poll::Ready(None) => {
                    self.done = true;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

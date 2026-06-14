use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use rust_agent_core::{AgentError, AgentResponseUpdate, Usage, FinishReason};
use serde::Deserialize;

/// Extended SseChunk covering full OpenAI/DeepSeek response fields
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SseChunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    system_fingerprint: Option<String>,
    #[serde(default)]
    choices: Vec<SseChoice>,
    #[serde(default)]
    usage: Option<SseUsage>,
}

#[derive(Debug, Deserialize)]
struct SseChoice {
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default)]
    delta: SseDelta,
}

#[derive(Debug, Deserialize, Default)]
struct SseDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<SseToolCall>>,
}

#[derive(Debug, Deserialize)]
struct SseToolCall {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<SseToolCallFunction>,
}

#[derive(Debug, Deserialize)]
struct SseToolCallFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u32>,
    #[serde(default)]
    prompt_cache_miss_tokens: Option<u32>,
    #[serde(default)]
    reasoning_tokens: Option<u32>,
}

/// Map a raw SSE chunk into one or more AgentResponseUpdate events.
/// A single chunk may produce multiple events (e.g., delta + usage + finish).
fn map_chunk(sse: SseChunk) -> Vec<AgentResponseUpdate> {
    let mut events = Vec::new();

    // Response metadata (from first chunk with id/model)
    if sse.id.is_some() || sse.model.is_some() {
        events.push(AgentResponseUpdate::ResponseMetadata {
            id: sse.id.clone(),
            model: sse.model.clone(),
        });
    }

    // Process choices
    for choice in &sse.choices {
        let delta = &choice.delta;

        // Text delta
        if let Some(ref content) = delta.content {
            if !content.is_empty() {
                events.push(AgentResponseUpdate::TextDelta {
                    delta: content.clone(),
                });
            }
        }

        // Reasoning delta (DeepSeek thinking)
        if let Some(ref reasoning) = delta.reasoning_content {
            if !reasoning.is_empty() {
                events.push(AgentResponseUpdate::ReasoningDelta {
                    delta: reasoning.clone(),
                });
            }
        }

        // Tool call deltas
        if let Some(ref tool_calls) = delta.tool_calls {
            for tc in tool_calls {
                let func = tc.function.as_ref();
                events.push(AgentResponseUpdate::ToolCallDelta {
                    index: tc.index,
                    id: tc.id.clone(),
                    name: func.and_then(|f| f.name.clone()),
                    arguments_delta: func
                        .and_then(|f| f.arguments.clone())
                        .unwrap_or_default(),
                });
            }
        }

        // Finish reason
        if let Some(ref reason) = choice.finish_reason {
            if !reason.is_empty() {
                let finish_reason = match reason.as_str() {
                    "stop" => FinishReason::Stop,
                    "length" => FinishReason::Length,
                    "tool_calls" => FinishReason::ToolCalls,
                    "content_filter" => FinishReason::ContentFilter,
                    other => FinishReason::Other(other.to_string()),
                };

                let usage = sse.usage.as_ref().map(|u| Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                    prompt_cache_hit_tokens: u.prompt_cache_hit_tokens,
                    prompt_cache_miss_tokens: u.prompt_cache_miss_tokens,
                    reasoning_tokens: u.reasoning_tokens,
                });

                events.push(AgentResponseUpdate::Finish {
                    finish_reason,
                    usage,
                });
            }
        }
    }

    // Usage-only event (when usage is present but no finish_reason)
    // This handles the final chunk case where usage appears without a delta
    if sse.usage.is_some() {
        let has_finish = sse.choices.iter().any(|c| c.finish_reason.is_some());
        if !has_finish {
            let u = sse.usage.as_ref().unwrap();
            events.push(AgentResponseUpdate::Usage {
                usage: Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                    prompt_cache_hit_tokens: u.prompt_cache_hit_tokens,
                    prompt_cache_miss_tokens: u.prompt_cache_miss_tokens,
                    reasoning_tokens: u.reasoning_tokens,
                },
            });
        }
    }

    events
}

/// SseStream — custom Stream implementation yielding AgentResponseUpdate events.
///
/// Buffers incoming bytes, splits on newlines, parses SSE data lines,
/// and emits individual AgentResponseUpdate items via FIFO from each parsed chunk.
pub struct SseStream<S> {
    inner: S,
    buffer: Vec<u8>,
    pending: std::vec::IntoIter<AgentResponseUpdate>,
    done: bool,
}

impl<S> SseStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Send + Unpin + 'static,
{
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            pending: Vec::new().into_iter(),
            done: false,
        }
    }
}

impl<S> Stream for SseStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Send + Unpin + 'static,
{
    type Item = Result<AgentResponseUpdate, AgentError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Drain pending events first
        if let Some(update) = self.pending.next() {
            return Poll::Ready(Some(Ok(update)));
        }

        loop {
            if self.done {
                return Poll::Ready(None);
            }

            // Try to extract a complete line from the buffer
            if let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
                let line_bytes = self.buffer.drain(..=pos).collect::<Vec<_>>();
                let line = String::from_utf8_lossy(&line_bytes);
                let trimmed = line.trim();

                if let Some(data) = trimmed.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        self.done = true;
                        return Poll::Ready(None);
                    }

                    match serde_json::from_str::<SseChunk>(data) {
                        Ok(chunk) => {
                            let mut events = map_chunk(chunk);
                            if !events.is_empty() {
                                // Return first event, store rest
                                let first = events.remove(0);
                                self.pending = events.into_iter();
                                return Poll::Ready(Some(Ok(first)));
                            }
                            // No events from this chunk, continue
                            continue;
                        }
                        Err(e) => {
                            return Poll::Ready(Some(Err(AgentError::StreamError(
                                format!("SSE parse error: {}", e),
                            ))));
                        }
                    }
                }
                // Non-data lines are silently skipped
                continue;
            }

            // Need more data from inner stream
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

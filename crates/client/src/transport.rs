use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use rust_agent_core::{AgentError, ChatStreamChunk, ToolCallDelta};
use serde::Deserialize;

/// Raw SSE chunk from the chat completion streaming response.
///
/// Matches OpenAI/DeepSeek delta JSON format:
/// ```json
/// {
///   "choices": [{
///     "delta": {
///       "content": "Hello",
///       "reasoning_content": "...",    // DeepSeek only
///       "tool_calls": [{ "index": 0, "id": "...", "function": { "name": "...", "arguments": "..." } }]
///     }
///   }]
/// }
/// ```
#[derive(Debug, Deserialize)]
struct SseChunk {
    choices: Vec<SseChoice>,
}

#[derive(Debug, Deserialize)]
struct SseChoice {
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
    function: Option<SseToolCallFunction>,
}

#[derive(Debug, Deserialize)]
struct SseToolCallFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

fn map_chunk(sse: SseChunk) -> ChatStreamChunk {
    let delta = sse.choices.into_iter().next().map(|c| c.delta).unwrap_or_default();

    let tool_call_delta = delta.tool_calls.and_then(|tc| {
        tc.into_iter().next().map(|t| {
            let func = t.function;
            ToolCallDelta {
                index: t.index,
                id: t.id,
                name: func.as_ref().and_then(|f| f.name.clone()),
                arguments_delta: func.and_then(|f| f.arguments),
            }
        })
    });

    ChatStreamChunk {
        text_delta: delta.content.filter(|s| !s.is_empty()),
        tool_call_delta,
        reasoning_delta: delta.reasoning_content.filter(|s| !s.is_empty()),
    }
}

/// Internal stream implementation wrapping reqwest byte stream with SSE parsing.
pub struct SseStream<S> {
    inner: S,
    buffer: Vec<u8>,
    done: bool,
}

impl<S> SseStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Send + Unpin + 'static,
{
    pub fn new(inner: S) -> Self {
        Self { inner, buffer: Vec::new(), done: false }
    }
}

impl<S> Stream for SseStream<S>
where
    S: Stream<Item = reqwest::Result<Bytes>> + Send + Unpin + 'static,
{
    type Item = Result<ChatStreamChunk, AgentError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
                            return Poll::Ready(Some(Ok(map_chunk(chunk))));
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

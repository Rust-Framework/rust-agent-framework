use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream,
    ChatMessage, Content, ErrorContent, FinishReason,
    IAgent, ISession, MessageRole, ResponseMetadata, Result,
    ToolCalledContent, ToolCallingContent, ToolCall, ITool,
};
use chrono::Utc;
use tokio::sync::mpsc;

/// ToolLoopAgent — implements the auto tool-calling loop.
///
/// Wraps an inner IAgent, intercepts ToolCallingContent, executes tools,
/// injects ToolCalledContent, and feeds results back to the inner agent.
pub struct ToolLoopAgent {
    id: AgentId,
    metadata: AgentMetadata,
    inner: Arc<dyn IAgent>,
    tools: Vec<Arc<dyn ITool>>,
    max_rounds: usize,
}

impl ToolLoopAgent {
    pub fn new(
        name: impl Into<String>,
        inner: Arc<dyn IAgent>,
        tools: Vec<Arc<dyn ITool>>,
    ) -> Self {
        let name = name.into();
        Self {
            id: AgentId::new(&name),
            metadata: AgentMetadata {
                agent_type: "ToolLoopAgent".to_string(),
                key: name.clone(),
                description: format!("Tool loop wrapping {}", inner.id()),
            },
            inner,
            tools,
            max_rounds: 10,
        }
    }

    pub fn with_max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds;
        self
    }
}

/// State machine for the tool-loop unfold stream.
///
/// - **Looping**: call inner agent with messages. Text deltas are forwarded
///   in real time through an mpsc channel; tool-call chunks are buffered
///   for post-processing after the stream ends.
/// - **Streaming**: drain remaining items from the mpsc receiver (text was
///   already forwarded by the spawned task during Looping phase).
/// - **Done**: stream ended.
enum LoopState {
    Looping {
        messages: Vec<ChatMessage>,
        round: usize,
    },
    Streaming {
        rx: mpsc::Receiver<Result<AgentResponseResult>>,
        on_done: Box<LoopState>,
    },
    Done,
}

#[async_trait]
impl IAgent for ToolLoopAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let inner = Arc::clone(&self.inner);
        let tools = Arc::new(self.tools.clone());
        let max_rounds = self.max_rounds;

        let initial_state = LoopState::Looping {
            messages,
            round: 0,
        };

        let stream = futures_util::stream::unfold(
            initial_state,
            move |state| {
                let inner = Arc::clone(&inner);
                let tools = Arc::clone(&tools);
                let session = session.clone();
                let options = options.clone();

                async move {
                    match state {
                        LoopState::Done => None,

                        LoopState::Streaming { mut rx, on_done } => {
                            match rx.recv().await {
                                Some(Ok(chunk)) => {
                                    // Check for loop-continuation signal
                                    if chunk.finish_reason == Some(FinishReason::ToolCalls)
                                        && chunk.contents.is_empty()
                                    {
                                        Some((Ok(chunk), *on_done))
                                    } else {
                                        Some((Ok(chunk), LoopState::Streaming { rx, on_done }))
                                    }
                                }
                                Some(Err(e)) => Some((Err(e), LoopState::Done)),
                                None => None,
                            }
                        }

                        LoopState::Looping { messages, round } => {
                            if round >= max_rounds {
                                let err_result = AgentResponseResult {
                                    id: None,
                                    model: None,
                                    finish_reason: Some(FinishReason::Stop),
                                    contents: vec![Content::Error(ErrorContent {
                                        meta: ResponseMetadata {
                                            agent_id: None,
                                            model_id: None,
                                            executor_id: None,
                                            timestamp: Utc::now(),
                                            properties: Default::default(),
                                        },
                                        error_code: "max_rounds".to_string(),
                                        message: format!(
                                            "Tool loop reached max rounds ({})",
                                            max_rounds
                                        ),
                                    })],
                                    events: vec![],
                                };
                                return Some((Ok(err_result), LoopState::Done));
                            }

                            // Call inner agent.
                            let stream = match inner
                                .run(messages.clone(), session.clone(), options.clone())
                                .await
                            {
                                Ok(s) => s,
                                Err(e) => {
                                    let err_result = AgentResponseResult {
                                        id: None,
                                        model: None,
                                        finish_reason: None,
                                        contents: vec![Content::Error(ErrorContent {
                                            meta: ResponseMetadata {
                                                agent_id: None,
                                                model_id: None,
                                                executor_id: None,
                                                timestamp: Utc::now(),
                                                properties: Default::default(),
                                            },
                                            error_code: "inner_agent_error".to_string(),
                                            message: e.to_string(),
                                        })],
                                        events: vec![],
                                    };
                                    return Some((Ok(err_result), LoopState::Done));
                                }
                            };

                            // Forward stream immediately for typing effect.
                            // Text chunks pass through in real time; tool-call chunks
                            // are buffered for post-processing after the stream ends.
                            let (tx, mut rx) =
                                mpsc::channel::<Result<AgentResponseResult>>(256);

                            let tools_clone = Arc::clone(&tools);
                            let session_clone = session.clone();
                            tokio::spawn(async move {
                                let mut s = stream;
                                let mut round_text = String::new();
                                let mut tool_callings: Vec<ToolCallingContent> = Vec::new();
                                let mut buffered: Vec<AgentResponseResult> = Vec::new();
                                let mut stream_err: Option<rust_agent_core::AgentError> = None;

                                while let Some(item) = s.next().await {
                                    match item {
                                        Ok(chunk) => {
                                            for c in &chunk.contents {
                                                match c {
                                                    Content::Text(t) => {
                                                        round_text.push_str(&t.delta);
                                                    }
                                                    Content::ToolCalling(tc) => {
                                                        tool_callings.push(tc.clone());
                                                    }
                                                    _ => {}
                                                }
                                            }
                                            let has_tool = chunk
                                                .contents
                                                .iter()
                                                .any(|c| matches!(c, Content::ToolCalling(_)));
                                            if has_tool {
                                                buffered.push(chunk);
                                            } else {
                                                if tx.send(Ok(chunk)).await.is_err() {
                                                    return;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            stream_err = Some(e);
                                            break;
                                        }
                                    }
                                }

                                if let Some(e) = stream_err {
                                    let _ = tx.send(Err(e)).await;
                                    return;
                                }

                                // Note: assistant text persistence is now handled by
                                // ChatClientAgent Phase 3 (via on_invoked), not here.
                                // Tool interactions (tool_calls + results) are still
                                // persisted below for context in subsequent loop iterations.

                                if tool_callings.is_empty() {
                                    return;
                                }

                                // Execute tools in parallel
                                let meta = ResponseMetadata {
                                    agent_id: None,
                                    model_id: None,
                                    executor_id: None,
                                    timestamp: Utc::now(),
                                    properties: Default::default(),
                                };

                                let tool_futures: Vec<_> = tool_callings
                                    .iter()
                                    .map(|tc| {
                                        let tc = tc.clone();
                                        let tools = Arc::clone(&tools_clone);
                                        let meta = meta.clone();
                                        async move {
                                            match tools.iter().find(|t| t.name() == tc.name) {
                                                Some(tool) => match tool.execute(tc.arguments.clone()).await {
                                                    Ok(output) => ToolCalledContent {
                                                        meta,
                                                        call_id: tc.call_id.clone(),
                                                        result: Some(output),
                                                        error: None,
                                                    },
                                                    Err(e) => ToolCalledContent {
                                                        meta,
                                                        call_id: tc.call_id.clone(),
                                                        result: None,
                                                        error: Some(e.to_string()),
                                                    },
                                                },
                                                None => ToolCalledContent {
                                                    meta,
                                                    call_id: tc.call_id.clone(),
                                                    result: None,
                                                    error: Some(format!("Tool '{}' not found", tc.name)),
                                                },
                                            }
                                        }
                                    })
                                    .collect::<Vec<_>>();

                                let results = futures_util::future::join_all(tool_futures).await;
                                let tool_results: Vec<Content> =
                                    results.into_iter().map(Content::ToolCalled).collect();

                                // Persist tool interactions to session
                                if let Some(ref sess) = session_clone {
                                    let _ = sess
                                        .add_message(ChatMessage {
                                            role: MessageRole::Assistant,
                                            content: String::new(),
                                            name: None,
                                            tool_calls: Some(
                                                tool_callings.iter().map(|tc| ToolCall {
                                                    id: tc.call_id.clone(),
                                                    name: tc.name.clone(),
                                                    arguments: tc.arguments.clone(),
                                                }).collect(),
                                            ),
                                            tool_call_id: None,
                                        })
                                        .await;
                                    for tc in &tool_callings {
                                        let content = tool_results
                                            .iter()
                                            .find_map(|c| match c {
                                                Content::ToolCalled(tcd)
                                                    if tcd.call_id == tc.call_id =>
                                                    tcd.result.clone(),
                                                _ => None,
                                            })
                                            .unwrap_or_default();
                                        let _ = sess
                                            .add_message(ChatMessage::tool(content, &tc.call_id))
                                            .await;
                                    }
                                }

                                // Forward buffered tool-call chunks to the stream
                                for buffered_chunk in buffered {
                                    if tx.send(Ok(buffered_chunk)).await.is_err() {
                                        return;
                                    }
                                }

                                // Emit combined tool-call + result chunk
                                let mut all_contents: Vec<Content> = Vec::new();
                                if !round_text.is_empty() {
                                    all_contents.push(Content::Text(
                                        rust_agent_core::TextContent {
                                            meta: meta.clone(),
                                            delta: round_text,
                                        },
                                    ));
                                }
                                all_contents.extend(
                                    tool_callings.iter().map(|tc| Content::ToolCalling(tc.clone())),
                                );
                                all_contents.extend(tool_results);

                                let _ = tx.send(Ok(AgentResponseResult {
                                    id: None,
                                    model: None,
                                    finish_reason: None,
                                    contents: all_contents,
                                    events: vec![],
                                })).await;

                                // Signal loop continuation via empty chunk with ToolCalls reason
                                let _ = tx.send(Ok(AgentResponseResult {
                                    id: None,
                                    model: None,
                                    finish_reason: Some(FinishReason::ToolCalls),
                                    contents: vec![],
                                    events: vec![],
                                })).await;
                            });

                            // Return first streaming item → caller transparently
                            // drains the rest via LoopState::Streaming.
                            match rx.recv().await {
                                Some(Ok(chunk)) => {
                                    let next = LoopState::Streaming {
                                        rx,
                                        on_done: Box::new(LoopState::Looping {
                                            messages: Vec::new(),
                                            round: round + 1,
                                        }),
                                    };
                                    Some((Ok(chunk), next))
                                }
                                Some(Err(e)) => Some((Err(e), LoopState::Done)),
                                None => None,
                            }
                        }
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }

    fn get_subagent(&self, agent_id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.inner.get_subagent(agent_id)
    }

    fn list_subagents(&self) -> Vec<Arc<dyn IAgent>> {
        let mut subs = self.inner.list_subagents();
        subs.push(Arc::clone(&self.inner) as Arc<dyn IAgent>);
        subs
    }

    async fn reset(&self) -> Result<()> {
        self.inner.reset().await
    }
}

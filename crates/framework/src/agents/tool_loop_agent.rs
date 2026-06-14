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

    #[allow(dead_code)]
    fn find_tool(&self, name: &str) -> Option<&Arc<dyn ITool>> {
        self.tools.iter().find(|t| t.name() == name)
    }
}

/// State for the tool-loop unfold stream.
enum LoopState {
    /// Calling the inner agent and collecting its response.
    Looping {
        messages: Vec<ChatMessage>,
        round: usize,
    },
    /// Emitting the final (non-tool-calling) chunks from the last round.
    EmittingFinal {
        chunks: std::vec::IntoIter<AgentResponseResult>,
    },
    /// Stream has ended.
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

                        LoopState::EmittingFinal { mut chunks } => {
                            match chunks.next() {
                                Some(chunk) => Some((Ok(chunk), LoopState::EmittingFinal { chunks })),
                                None => None,
                            }
                        }

                        LoopState::Looping { messages, round } => {
                            if round >= max_rounds {
                                // Max rounds reached – build an error chunk and stop.
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

                            // Collect all chunks from the inner agent in this round.
                            let mut all_chunks: Vec<AgentResponseResult> = Vec::new();
                            {
                                let mut s = stream;
                                while let Some(item) = s.next().await {
                                    match item {
                                        Ok(chunk) => all_chunks.push(chunk),
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
                                                    error_code: "stream_error".to_string(),
                                                    message: e.to_string(),
                                                })],
                                                events: vec![],
                                            };
                                            return Some((Ok(err_result), LoopState::Done));
                                        }
                                    }
                                }
                            }

                            // Extract tool callings from all chunks.
                            let tool_callings: Vec<ToolCallingContent> = all_chunks
                                .iter()
                                .flat_map(|c| c.contents.iter())
                                .filter_map(|c| match c {
                                    Content::ToolCalling(tc) => Some(tc.clone()),
                                    _ => None,
                                })
                                .collect();

                            if tool_callings.is_empty() {
                                // No tool calls – emit all collected chunks as final.
                                let mut chunks_iter = all_chunks.into_iter();
                                match chunks_iter.next() {
                                    Some(first) => {
                                        Some((Ok(first), LoopState::EmittingFinal {
                                            chunks: chunks_iter,
                                        }))
                                    }
                                    None => None,
                                }
                            } else {
                                // Execute tools and build results.
                                let meta = ResponseMetadata {
                                    agent_id: None,
                                    model_id: None,
                                    executor_id: None,
                                    timestamp: Utc::now(),
                                    properties: Default::default(),
                                };

                                let mut tool_results: Vec<Content> = Vec::new();
                                for tc in &tool_callings {
                                    let result = match tools.iter().find(|t| t.name() == tc.name) {
                                        Some(tool) => {
                                            match tool.execute(tc.arguments.clone()).await {
                                                Ok(output) => ToolCalledContent {
                                                    meta: meta.clone(),
                                                    call_id: tc.call_id.clone(),
                                                    result: Some(output),
                                                    error: None,
                                                },
                                                Err(e) => ToolCalledContent {
                                                    meta: meta.clone(),
                                                    call_id: tc.call_id.clone(),
                                                    result: None,
                                                    error: Some(e.to_string()),
                                                },
                                            }
                                        }
                                        None => ToolCalledContent {
                                            meta: meta.clone(),
                                            call_id: tc.call_id.clone(),
                                            result: None,
                                            error: Some(format!(
                                                "Tool '{}' not found",
                                                tc.name
                                            )),
                                        },
                                    };
                                    tool_results.push(Content::ToolCalled(result));
                                }

                                // Build assistant message with tool_calls.
                                let tool_calls_for_msg: Vec<ToolCall> = tool_callings
                                    .iter()
                                    .map(|tc| ToolCall {
                                        id: tc.call_id.clone(),
                                        name: tc.name.clone(),
                                        arguments: tc.arguments.clone(),
                                    })
                                    .collect();

                                let mut new_messages = messages;
                                new_messages.push(ChatMessage {
                                    role: MessageRole::Assistant,
                                    content: String::new(),
                                    name: None,
                                    tool_calls: Some(tool_calls_for_msg),
                                    tool_call_id: None,
                                });

                                // Push tool result messages.
                                for tc in &tool_callings {
                                    let tool_result_content = tool_results.iter().find_map(|c| match c {
                                        Content::ToolCalled(tcd) if tcd.call_id == tc.call_id => {
                                            Some(tcd)
                                        }
                                        _ => None,
                                    });
                                    new_messages.push(ChatMessage {
                                        role: MessageRole::Tool,
                                        content: tool_result_content
                                            .and_then(|t| t.result.clone())
                                            .unwrap_or_default(),
                                        name: None,
                                        tool_calls: None,
                                        tool_call_id: Some(tc.call_id.clone()),
                                    });
                                }

                                // Build a chunk containing all tool calling + tool called content.
                                let mut all_contents: Vec<Content> = Vec::new();
                                all_contents.extend(
                                    tool_callings.into_iter().map(Content::ToolCalling),
                                );
                                all_contents.extend(tool_results);

                                let result = AgentResponseResult {
                                    id: None,
                                    model: None,
                                    finish_reason: None,
                                    contents: all_contents,
                                    events: vec![],
                                };

                                Some((
                                    Ok(result),
                                    LoopState::Looping {
                                        messages: new_messages,
                                        round: round + 1,
                                    },
                                ))
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

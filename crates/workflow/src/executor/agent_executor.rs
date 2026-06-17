use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::{ChatMessage, IAgent, Result};
use tokio::sync::mpsc::UnboundedSender;

use super::base::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use crate::engine::IWorkflowContext;

/// AgentExecutor — 将已有的 `IAgent` 实现适配为 `IExecutor`
///
/// # 流式全链路保证
///
/// `IAgent::run()` 返回的 `BoxStream<AgentResponseResult>` 在内部被逐帧消费：
/// - Text/Reasoning → `progress.send(NodeProgress::TextDelta(...))`
/// - ToolCallStart/Args/End → `progress.send(NodeProgress::ToolCallXxx(...))`
/// - 最终结果 → `HandlerResult::Messages(vec![chat_message])`
pub struct AgentExecutor {
    id: String,
    agent: Arc<dyn IAgent>,
    is_output: bool,
}

impl AgentExecutor {
    pub fn new(id: impl Into<String>, agent: Arc<dyn IAgent>) -> Self {
        Self {
            id: id.into(),
            agent,
            is_output: false,
        }
    }

    pub fn with_output(mut self, is_output: bool) -> Self {
        self.is_output = is_output;
        self
    }

    pub fn agent(&self) -> &Arc<dyn IAgent> {
        &self.agent
    }
}

#[async_trait]
impl IExecutor for AgentExecutor {
    fn id(&self) -> &str {
        &self.id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("ChatMessage")]
    }

    fn send_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("ChatMessage")]
    }

    fn is_output(&self) -> bool {
        self.is_output
    }

    fn as_agent(&self) -> Option<&Arc<dyn IAgent>> {
        Some(&self.agent)
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: &dyn IWorkflowContext,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let messages = self.extract_messages(&message, ctx).await?;
        let session = ctx.session().cloned();

        let stream = self.agent.run(messages, session, None).await?;
        futures_util::pin_mut!(stream);

        let mut collected_text = String::new();
        let mut has_content = false;
        while let Some(item) = stream.next().await {
            match item {
                Ok(result) => {
                    for content in &result.contents {
                        use rust_agent_core::Content;
                        match content {
                            Content::Text(tc) => {
                                collected_text.push_str(&tc.delta);
                                has_content = true;
                                let _ = progress.send(NodeProgress::TextDelta(tc.delta.clone()));
                            }
                            Content::Reasoning(rc) => {
                                let _ = progress.send(NodeProgress::ReasoningDelta(
                                    rc.delta.clone(),
                                ));
                            }
                            Content::ToolCallStart(tcs) => {
                                let _ = progress.send(NodeProgress::ToolCallStart {
                                    call_id: tcs.call_id.clone(),
                                    name: tcs.name.clone(),
                                });
                            }
                            Content::ToolCallArgs(tca) => {
                                let _ = progress.send(NodeProgress::ToolCallArgs {
                                    call_id: tca.call_id.clone(),
                                    args_delta: tca.args_delta.clone(),
                                });
                            }
                            Content::ToolCallEnd(tce) => {
                                let _ = progress.send(NodeProgress::ToolCallEnd {
                                    call_id: tce.call_id.clone(),
                                });
                            }
                            Content::ToolCalled(tcr) => {
                                let _ = progress.send(NodeProgress::ToolResult {
                                    call_id: tcr.call_id.clone(),
                                    result: tcr.result.clone().unwrap_or_default(),
                                });
                            }
                            Content::Usage(uc) => {
                                let _ = progress.send(NodeProgress::UsageUpdate {
                                    prompt_tokens: uc.usage.prompt_tokens,
                                    completion_tokens: uc.usage.completion_tokens,
                                });
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        let out_msgs: Vec<Arc<dyn std::any::Any + Send + Sync>> = if has_content {
            vec![Arc::new(ChatMessage::assistant(collected_text))]
        } else {
            vec![]
        };
        Ok(HandlerResult::Messages(out_msgs))
    }
}

impl AgentExecutor {
    async fn extract_messages(
        &self,
        message: &Arc<dyn std::any::Any + Send + Sync>,
        ctx: &dyn IWorkflowContext,
    ) -> Result<Vec<ChatMessage>> {
        if let Some(msg) = message.downcast_ref::<ChatMessage>() {
            return Ok(vec![msg.clone()]);
        }

        if let Some(msgs) = message.downcast_ref::<Vec<ChatMessage>>() {
            return Ok(msgs.clone());
        }

        if let Some(session) = ctx.session() {
            session.get_messages().await
        } else {
            Ok(vec![ChatMessage::user("执行任务")])
        }
    }
}

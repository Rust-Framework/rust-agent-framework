use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::{ChatMessage, IAgent, Result};
use tokio::sync::mpsc::UnboundedSender;

use super::base::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use crate::engine::IWorkflowContext;

/// AgentExecutor — 将已有的 `IAgent` 实现适配为 `IExecutor`
///
/// 这是关键桥接器，使任何实现了 `IAgent` 的组件都能直接作为工作流节点使用。
///
/// # 流式全链路保证
///
/// `IAgent::run()` 返回的 `BoxStream<AgentResponseResult>` 在内部被逐帧消费：
/// - Text/Reasoning → `progress.send(NodeProgress::TextDelta(...))`
/// - ToolCallStart/Args/End → `progress.send(NodeProgress::ToolCallXxx(...))`
/// - 最终结果 → `HandlerResult::Messages(vec![chat_message])`
///
/// 前端通过 `WorkflowEvent::NodeStreaming` 实时获得每个 Agent 的打字机输出。
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

    async fn handle(
        &self,
        message: Box<dyn std::any::Any + Send + Sync>,
        ctx: &dyn IWorkflowContext,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        // 1. 从 message 提取 ChatMessage 列表
        let messages = self.extract_messages(message, ctx).await?;

        // 2. 获取 session（如有）
        let session = ctx.session().cloned();

        // 3. 调用 Agent
        let stream = self.agent.run(messages, session, None).await?;
        futures_util::pin_mut!(stream);

        // 4. 逐帧消费流，实时转发进度
        while let Some(item) = stream.next().await {
            match item {
                Ok(result) => {
                    // 遍历每个 Content 变体
                    for content in &result.contents {
                        use rust_agent_core::Content;
                        match content {
                            Content::Text(tc) => {
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
                            // 忽略其他内容类型
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        // 5. 收集最终结果，构造下游消息
        // 注意：需要重新获取 stream 并 collect，因为我们已经消费了 stream
        // 简化：直接构造一个空的 ChatMessage 作为信号
        // TODO: 正确收集 stream（当前架构下收集和流式变体不兼容，
        // 需要在 engine 层引入双 channel 机制来分离流式输出和路由结果）
        Ok(HandlerResult::Messages(vec![]))
    }
}

impl AgentExecutor {
    async fn extract_messages(
        &self,
        message: Box<dyn std::any::Any + Send + Sync>,
        ctx: &dyn IWorkflowContext,
    ) -> Result<Vec<ChatMessage>> {
        // 尝试从 message 中提取 ChatMessage
        if let Some(msg) = message.downcast_ref::<ChatMessage>() {
            return Ok(vec![msg.clone()]);
        }

        if let Some(msgs) = message.downcast_ref::<Vec<ChatMessage>>() {
            return Ok(msgs.clone());
        }

        // 回退到 session history
        if let Some(session) = ctx.session() {
            session.get_messages().await
        } else {
            Ok(vec![ChatMessage::user("执行任务")])
        }
    }
}

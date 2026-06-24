use std::sync::Arc;

use rust_agent_core::{
    AgentResponse, AgentResponseResult, ChatMessage, Content, ISession,
    MessageRole, Result, ToolCall,
};

use crate::bundle::build_turn_transcript;

use super::chat_client::ChatClientAgent;
use super::proxy::AgentProxy;

impl ChatClientAgent {
    /// 创建非阻塞 post-invocation 处理器。
    ///
    /// 通过 channel 分叉，将流式响应同时发送给消费者和后台任务。
    /// 后台任务收集完整响应后，调用所有 context provider 的 `on_invoked` 钩子，
    /// 并将 assistant/tool 消息持久化到 session。
    pub(super) fn spawn_post_invocation_handler(
        &self,
        converted: impl futures_core::Stream<Item = Result<AgentResponseResult>> + Send + 'static,
        session: Option<Arc<dyn ISession>>,
        request_messages: Vec<ChatMessage>,
        caller_messages: Vec<ChatMessage>,
    ) -> impl futures_core::Stream<Item = Result<AgentResponseResult>> + Send + 'static {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let providers = Arc::new(self.context_providers.clone());
        let session_for_invoked = session;
        let agent_id_proxy = self.id.clone();
        let agent_meta_proxy = self.metadata.clone();
        let chat_client_proxy = self.chat_client.clone();

        tokio::spawn(async move {
            let mut collected: Vec<Result<AgentResponseResult>> = Vec::new();
            while let Some(chunk) = rx.recv().await {
                collected.push(chunk);
            }
            if collected.is_empty() {
                return;
            }

            let mut text = String::new();
            let mut tool_calls = Vec::new();
            let mut tool_results: Vec<(String, Option<String>, Option<String>)> = Vec::new();
            let mut source_agent_id = None;
            let mut finish_reason = None;
            let flat_chunks: Vec<AgentResponseResult> =
                collected.iter().flatten().cloned().collect();
            for chunk in collected.iter().flatten() {
                if chunk.finish_reason.is_some() {
                    finish_reason = chunk.finish_reason.clone();
                }
                for content in &chunk.contents {
                    if let Content::Text(c) = content {
                        text.push_str(&c.delta);
                    }
                    if let Content::ToolCalling(c) = content {
                        let args = match &c.arguments {
                            serde_json::Value::String(_) => c.arguments.clone(),
                            other => serde_json::Value::String(other.to_string()),
                        };
                        tool_calls.push(ToolCall {
                            id: c.call_id.clone(),
                            name: c.name.clone(),
                            arguments: args,
                        });
                        if source_agent_id.is_none() {
                            source_agent_id = c.meta.agent_id.clone();
                        }
                    }
                    if let Content::ToolCalled(c) = content {
                        tool_results.push((c.call_id.clone(), c.result.clone(), c.error.clone()));
                    }
                }
            }
            let mut tool_result_messages = Vec::new();
            for tc in &tool_calls {
                let content = tool_results
                    .iter()
                    .find(|(id, _, _)| id == &tc.id)
                    .and_then(|(_, result, error)| error.clone().or_else(|| result.clone()))
                    .unwrap_or_default();
                tool_result_messages.push(ChatMessage::tool(content, &tc.id));
            }
            let turn_transcript = build_turn_transcript(&caller_messages, &flat_chunks);
            let response = AgentResponse {
                id: None,
                model: None,
                text,
                reasoning_text: None,
                tool_calls,
                tool_messages: tool_result_messages,
                turn_transcript,
                finish_reason,
                usage: None,
                source_agent_id,
            };
            let proxy = AgentProxy {
                id: agent_id_proxy,
                metadata: agent_meta_proxy,
                chat_client: chat_client_proxy,
            };

            if let Some(ref sess) = session_for_invoked {
                let invoked_ctx = rust_agent_core::InvokedContext {
                    agent: &proxy,
                    session: sess.as_ref(),
                    request_messages: &request_messages,
                    response: Some(&response),
                    error: None,
                };
                for provider in providers.iter() {
                    if let Err(e) = provider.on_invoked(&invoked_ctx).await {
                        tracing::warn!(provider = %provider.name(), error = %e, "on_invoked failed");
                    }
                }

                if !response.turn_transcript.is_empty() {
                    let non_user: Vec<ChatMessage> = response
                        .turn_transcript
                        .iter()
                        .filter(|m| m.role != MessageRole::User)
                        .cloned()
                        .collect();
                    if !non_user.is_empty() {
                        if let Err(e) = sess.add_messages_batch(&non_user).await {
                            tracing::warn!(error = %e, "Failed to persist turn transcript to session");
                        }
                    }
                } else if !response.tool_calls.is_empty() {
                    if let Err(e) = sess.add_message(ChatMessage::assistant_with_tools(
                        response.text.clone(),
                        response.tool_calls.clone(),
                    ))
                    .await
                    {
                        tracing::warn!(error = %e, "Failed to persist assistant+tool_calls message to session");
                    }
                    for tm in &response.tool_messages {
                        if let Err(e) = sess.add_message(tm.clone()).await {
                            tracing::warn!(error = %e, "Failed to persist tool result message to session");
                        }
                    }
                } else if !response.text.is_empty() {
                    if let Err(e) = sess.add_message(ChatMessage::assistant(response.text.clone())).await
                    {
                        tracing::warn!(error = %e, "Failed to persist assistant message to session");
                    }
                }
            }
        });

        futures_util::StreamExt::inspect(converted, move |chunk| {
            if let Ok(ref c) = chunk {
                if tx.send(Ok(c.clone())).is_err() {
                    tracing::warn!(
                        "Post-invocation channel closed — context provider notifications may be lost"
                    );
                }
            }
        })
    }
}

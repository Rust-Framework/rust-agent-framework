use std::pin::Pin;

use futures_core::Stream;

use crate::{AgentId, AgentResponse, AgentResponseResult, Content, Result, ToolCall};

/// 装箱的可发送流的类型别名。
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

/// 将智能体流收集为单个聚合的 AgentResponse。
pub async fn collect_agent_response(
    mut stream: BoxStream<'static, Result<AgentResponseResult>>,
) -> Result<AgentResponse> {
    use futures_util::StreamExt;

    let mut text = String::new();
    let mut reasoning_text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut source_agent_id: Option<AgentId> = None;
    let mut finish_reason = None;
    let mut usage = None;
    let mut id = None;
    let mut model = None;

    while let Some(result) = stream.next().await {
        let chunk = result?;

        if chunk.id.is_some() {
            id = chunk.id;
        }
        if chunk.model.is_some() {
            model = chunk.model;
        }
        if chunk.finish_reason.is_some() {
            finish_reason = chunk.finish_reason;
        }

        for content in chunk.contents {
            match content {
                Content::Text(c) => {
                    text.push_str(&c.delta);
                }
                Content::Reasoning(c) => {
                    reasoning_text.push_str(&c.delta);
                }
                Content::ToolCalling(c) => {
                    tool_calls.push(ToolCall {
                        id: c.call_id,
                        name: c.name,
                        arguments: c.arguments,
                    });
                    // Track source_agent_id from meta
                    if let Some(aid) = c.meta.agent_id {
                        source_agent_id = Some(aid);
                    }
                }
                Content::Usage(c) => {
                    usage = Some(c.usage);
                }
                _ => {}
            }
        }
    }

    Ok(AgentResponse {
        id,
        model,
        text,
        reasoning_text: if reasoning_text.is_empty() {
            None
        } else {
            Some(reasoning_text)
        },
        tool_calls,
        tool_messages: Vec::new(),
        turn_transcript: Vec::new(),
        finish_reason,
        usage,
        source_agent_id,
    })
}

//! Curator 合并的选择性上下文投影。
//!
//! 过滤 Main Agent 系统提示与提供器广告，仅保留事实性消息与知识工具结果。

use rust_agent_core::{ChatMessage, MessageRole, ToolCall, AgentResponseResult, Content, FinishReason};

/// Session state key for accumulated consolidation projection.
pub const PROJECTION_STATE_KEY: &str = "SuperBrainContextProvider_projection";

const KNOWLEDGE_TOOLS: &[&str] = &[
    "web_fetch",
    "web_search",
    "read_skill_resource",
    "load_skill",
    "read_file",
    "search_file",
];

const SKIP_TOOLS: &[&str] = &["echo", "add"];

/// 工具的结果是否对记忆合并有用。
pub fn is_knowledge_tool(name: &str) -> bool {
    KNOWLEDGE_TOOLS.contains(&name)
}

fn is_skip_tool(name: &str) -> bool {
    SKIP_TOOLS.contains(&name)
}

/// 为 MemoryAgent 投影原始消息切片：丢弃系统消息，过滤不相关工具。
pub fn project_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        match msg.role {
            MessageRole::System => {
                i += 1;
            }
            MessageRole::User => {
                out.push(msg.clone());
                i += 1;
            }
            MessageRole::Assistant => {
                if let Some(ref tcs) = msg.tool_calls {
                    if tcs.is_empty() {
                        if !msg.content.trim().is_empty() {
                            out.push(msg.clone());
                        }
                        i += 1;
                        continue;
                    }
                    let relevant: Vec<&ToolCall> = tcs
                        .iter()
                        .filter(|tc| !is_skip_tool(&tc.name))
                        .collect();
                    if relevant.is_empty() {
                        i += 1;
                        continue;
                    }
                    let filtered_calls: Vec<ToolCall> =
                        relevant.iter().map(|tc| (*tc).clone()).collect();
                    let assistant = ChatMessage::assistant_with_tools(
                        msg.content.clone(),
                        filtered_calls.clone(),
                    );
                    out.push(assistant);
                    i += 1;
                    for tc in &filtered_calls {
                        if is_knowledge_tool(&tc.name) {
                            if i < messages.len()
                                && messages[i].role == MessageRole::Tool
                                && messages[i].tool_call_id.as_deref() == Some(&tc.id)
                            {
                                out.push(messages[i].clone());
                                i += 1;
                            }
                        } else if i < messages.len()
                            && messages[i].role == MessageRole::Tool
                            && messages[i].tool_call_id.as_deref() == Some(&tc.id)
                        {
                            // Skip non-knowledge tool result but consume the message.
                            i += 1;
                        }
                    }
                } else if !msg.content.trim().is_empty() {
                    out.push(msg.clone());
                    i += 1;
                } else {
                    i += 1;
                }
            }
            MessageRole::Tool => {
                // Orphan tool message — include only if from a knowledge tool.
                let name = msg.name.as_deref().unwrap_or("");
                if is_knowledge_tool(name) {
                    out.push(msg.clone());
                }
                i += 1;
            }
        }
    }
    out
}

fn messages_tail_equal(a: &[ChatMessage], b: &[ChatMessage]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.role == y.role
                && x.content == y.content
                && x.tool_call_id == y.tool_call_id
                && x.name == y.name
        })
}

/// 合并先前投影历史与当前轮次（仅追加，按尾部去重）。
pub fn merge_memory_projection(
    history: &[ChatMessage],
    turn: &[ChatMessage],
) -> Vec<ChatMessage> {
    if history.is_empty() {
        return turn.to_vec();
    }
    if turn.is_empty() {
        return history.to_vec();
    }
    // If turn is already contained at the tail of history, return history as-is.
    if history.len() >= turn.len() {
        let start = history.len() - turn.len();
        if messages_tail_equal(&history[start..], turn) {
            return history.to_vec();
        }
    }
    let mut merged = history.to_vec();
    merged.extend(turn.iter().cloned());
    merged
}

/// 构建合并输入：投影历史 + 投影当前轮次。
pub fn build_consolidation_context(
    memory_projection: &[ChatMessage],
    turn_transcript: &[ChatMessage],
) -> Vec<ChatMessage> {
    let projected_turn = project_messages(turn_transcript);
    merge_memory_projection(memory_projection, &projected_turn)
}

/// 从流块和调用者消息重建当前轮次的事实转录。
pub fn build_turn_transcript(
    caller_messages: &[ChatMessage],
    chunks: &[AgentResponseResult],
) -> Vec<ChatMessage> {
    let mut transcript: Vec<ChatMessage> = caller_messages
        .iter()
        .filter(|m| m.role == MessageRole::User)
        .cloned()
        .collect();

    let mut text = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut tool_results: Vec<(String, Option<String>, Option<String>)> = Vec::new();

    let flush_tool_round = |transcript: &mut Vec<ChatMessage>,
                            text: &mut String,
                            tool_calls: &mut Vec<ToolCall>,
                            tool_results: &mut Vec<(String, Option<String>, Option<String>)>| {
        if tool_calls.is_empty() {
            return;
        }
        transcript.push(ChatMessage::assistant_with_tools(
            std::mem::take(text),
            tool_calls.clone(),
        ));
        for tc in tool_calls.iter() {
            let content = tool_results
                .iter()
                .find(|(id, _, _)| id == &tc.id)
                .and_then(|(_, result, error)| error.clone().or_else(|| result.clone()))
                .unwrap_or_default();
            let mut tool_msg = ChatMessage::tool(content, &tc.id);
            tool_msg.name = Some(tc.name.clone());
            transcript.push(tool_msg);
        }
        tool_calls.clear();
        tool_results.clear();
    };

    for chunk in chunks {
        for content in &chunk.contents {
            match content {
                Content::Text(c) => text.push_str(&c.delta),
                Content::ToolCalling(c) => {
                    tool_calls.push(ToolCall {
                        id: c.call_id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    });
                }
                Content::ToolCalled(c) => {
                    tool_results.push((c.call_id.clone(), c.result.clone(), c.error.clone()));
                }
                _ => {}
            }
        }
        if matches!(
            chunk.finish_reason,
            Some(FinishReason::ToolCalls)
        ) {
            flush_tool_round(
                &mut transcript,
                &mut text,
                &mut tool_calls,
                &mut tool_results,
            );
        }
    }

    flush_tool_round(
        &mut transcript,
        &mut text,
        &mut tool_calls,
        &mut tool_results,
    );

    if !text.trim().is_empty() {
        transcript.push(ChatMessage::assistant(text));
    }

    transcript
}

/// Load consolidation projection from session provider state.
pub fn load_projection(session: &dyn rust_agent_core::ISession) -> Vec<ChatMessage> {
    session
        .get_provider_state(PROJECTION_STATE_KEY)
        .ok()
        .and_then(|v| serde_json::from_value::<Vec<ChatMessage>>(v).ok())
        .unwrap_or_default()
}

/// Save consolidation projection to session provider state.
pub fn save_projection(
    session: &dyn rust_agent_core::ISession,
    projection: &[ChatMessage],
) -> rust_agent_core::Result<()> {
    let value = serde_json::to_value(projection)
        .map_err(|e| rust_agent_core::AgentError::Serialize(e.to_string()))?;
    session.set_provider_state(PROJECTION_STATE_KEY, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_system_and_echo_tools() {
        let messages = vec![
            ChatMessage::system("Memory-first principle"),
            ChatMessage::user("learn prompt engineering"),
            ChatMessage::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "1".into(),
                    name: "echo".into(),
                    arguments: serde_json::json!({"text": "hi"}),
                }],
            ),
            ChatMessage::tool("hi", "1"),
            ChatMessage::assistant("summary"),
        ];
        let projected = project_messages(&messages);
        assert_eq!(projected.len(), 2);
        assert_eq!(projected[0].role, MessageRole::User);
        assert_eq!(projected[1].role, MessageRole::Assistant);
    }

    #[test]
    fn keeps_web_fetch_chain() {
        let messages = vec![
            ChatMessage::user("search and learn"),
            ChatMessage::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "w1".into(),
                    name: "web_fetch".into(),
                    arguments: serde_json::json!({"url": "https://example.com"}),
                }],
            ),
            ChatMessage::tool(r#"{"ok":true,"data":{"content":"tips"}}"#, "w1"),
            ChatMessage::assistant("learned tips"),
        ];
        let projected = project_messages(&messages);
        assert_eq!(projected.len(), 4);
        assert_eq!(projected[2].role, MessageRole::Tool);
    }
}

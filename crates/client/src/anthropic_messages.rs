//! Convert RAF `ChatMessage` / OpenAI tool defs → Anthropic Messages API format.

use rust_agent_core::{ChatMessage, MessageRole, ToolCall};

/// Extract system prompt and non-system messages for Anthropic `/v1/messages`.
pub fn convert_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<serde_json::Value>) {
    let system_parts: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == MessageRole::System)
        .map(|m| m.content.as_str())
        .filter(|s| !s.is_empty())
        .collect();

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    let mut out: Vec<serde_json::Value> = Vec::new();
    let mut pending_tool_results: Vec<serde_json::Value> = Vec::new();

    let flush_tool_results = |pending: &mut Vec<serde_json::Value>, out: &mut Vec<serde_json::Value>| {
        if pending.is_empty() {
            return;
        }
        out.push(serde_json::json!({
            "role": "user",
            "content": std::mem::take(pending),
        }));
    };

    for msg in messages.iter().filter(|m| m.role != MessageRole::System) {
        if msg.role == MessageRole::Tool {
            let tool_use_id = msg
                .tool_call_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            pending_tool_results.push(serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": msg.content,
            }));
            continue;
        }

        flush_tool_results(&mut pending_tool_results, &mut out);

        match msg.role {
            MessageRole::User => {
                out.push(serde_json::json!({
                    "role": "user",
                    "content": msg.content,
                }));
            }
            MessageRole::Assistant => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if !msg.content.is_empty() {
                    blocks.push(serde_json::json!({
                        "type": "text",
                        "text": msg.content,
                    }));
                }
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        blocks.push(tool_use_block(tc));
                    }
                }
                if blocks.is_empty() {
                    blocks.push(serde_json::json!({ "type": "text", "text": "" }));
                }
                out.push(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }));
            }
            MessageRole::System | MessageRole::Tool => unreachable!(),
        }
    }

    flush_tool_results(&mut pending_tool_results, &mut out);
    (system, out)
}

fn tool_use_block(tc: &ToolCall) -> serde_json::Value {
    let input = match &tc.arguments {
        serde_json::Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| {
            serde_json::json!({ "raw": s })
        }),
        other => other.clone(),
    };
    serde_json::json!({
        "type": "tool_use",
        "id": tc.id,
        "name": tc.name,
        "input": input,
    })
}

/// OpenAI `tools` array → Anthropic `tools` with `input_schema`.
pub fn convert_tools(openai_tools: &[serde_json::Value]) -> Vec<serde_json::Value> {
    openai_tools
        .iter()
        .filter_map(|tool| {
            let func = tool.get("function")?;
            let name = func.get("name")?.as_str()?;
            let description = func
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let input_schema = func.get("parameters").cloned().unwrap_or_else(|| {
                serde_json::json!({ "type": "object", "properties": {} })
            });
            Some(serde_json::json!({
                "name": name,
                "description": description,
                "input_schema": input_schema,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ToolCall;

    #[test]
    fn system_extracted_to_top_level() {
        let messages = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("hi"),
        ];
        let (system, msgs) = convert_messages(&messages);
        assert_eq!(system.as_deref(), Some("You are helpful"));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn tool_results_batch_into_user_message() {
        let messages = vec![
            ChatMessage::user("go"),
            ChatMessage::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "c1".into(),
                    name: "run".into(),
                    arguments: serde_json::json!({}),
                }],
            ),
            ChatMessage::tool("ok", "c1"),
        ];
        let (_, msgs) = convert_messages(&messages);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        let results = msgs[2]["content"].as_array().unwrap();
        assert_eq!(results[0]["type"], "tool_result");
        assert_eq!(results[0]["tool_use_id"], "c1");
    }

    #[test]
    fn converts_openai_tools() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": { "type": "object", "properties": { "city": { "type": "string" } } }
            }
        })];
        let anthropic = convert_tools(&tools);
        assert_eq!(anthropic[0]["name"], "get_weather");
        assert_eq!(anthropic[0]["input_schema"]["type"], "object");
    }
}

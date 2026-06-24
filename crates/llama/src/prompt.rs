use llama_gguf::engine::{ChatTemplate, Engine};
use rust_agent_core::{ChatMessage, MessageRole};

/// Prompt formatting style detected from model metadata.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptStyle {
    /// Use llama-gguf [`ChatTemplate`] helpers.
    Template(ChatTemplate),
    /// Gemma family: `<start_of_turn>user` / `<start_of_turn>model`.
    Gemma,
}

impl PromptStyle {
    pub fn stop_patterns<'a>(self, template: &'a ChatTemplate) -> &'a [&'a str] {
        match &self {
            PromptStyle::Gemma => &["<end_of_turn>", "<start_of_turn>user"],
            PromptStyle::Template(_) => template.stop_patterns(),
        }
    }
}

/// Detect the best prompt style from GGUF metadata and llama-gguf template detection.
pub fn detect_prompt_style(engine: &Engine) -> PromptStyle {
    if let Some(gguf) = engine.gguf() {
        if let Some(template) = gguf.data.get_string("tokenizer.chat_template") {
            if template.contains("start_of_turn") {
                return PromptStyle::Gemma;
            }
        }
        if let Some(arch) = gguf.data.get_string("general.architecture") {
            let arch = arch.to_lowercase();
            if arch.starts_with("gemma") {
                return PromptStyle::Gemma;
            }
        }
    }
    PromptStyle::Template(engine.chat_template().clone())
}

/// 将 `ChatMessage` 历史格式化为 llama-gguf 可推理的 prompt 字符串。
pub fn build_prompt(messages: &[ChatMessage], style: &PromptStyle) -> String {
    let system_content: String = messages
        .iter()
        .filter(|m| m.role == MessageRole::System)
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let dialog: Vec<&ChatMessage> = messages
        .iter()
        .filter(|m| m.role != MessageRole::System)
        .collect();

    if dialog.is_empty() {
        return String::new();
    }

    match style {
        PromptStyle::Gemma => build_gemma_prompt(&system_content, &dialog),
        PromptStyle::Template(ChatTemplate::None) => {
            build_plain_prompt(&system_content, &dialog)
        }
        PromptStyle::Template(template) => {
            build_templated_prompt(template, &system_content, &dialog)
        }
    }
}

/// Returns the byte index of the earliest stop pattern in `text`, if any.
pub fn find_stop(text: &str, style: &PromptStyle, template: &ChatTemplate) -> Option<usize> {
    let patterns = match style {
        PromptStyle::Gemma => &["<end_of_turn>", "<start_of_turn>user"][..],
        PromptStyle::Template(_) => template.stop_patterns(),
    };
    patterns
        .iter()
        .filter_map(|pattern| text.find(pattern))
        .min()
}

fn build_gemma_prompt(system_content: &str, dialog: &[&ChatMessage]) -> String {
    let mut out = String::new();
    let mut first_user = true;

    for msg in dialog {
        match msg.role {
            MessageRole::User => {
                out.push_str("<start_of_turn>user\n");
                if first_user && !system_content.is_empty() {
                    out.push_str(system_content);
                    out.push_str("\n\n");
                    first_user = false;
                }
                out.push_str(&msg.content);
                out.push_str("<end_of_turn>\n");
            }
            MessageRole::Assistant => {
                out.push_str("<start_of_turn>model\n");
                out.push_str(&msg.content);
                out.push_str("<end_of_turn>\n");
            }
            MessageRole::Tool => {
                let tool_id = msg.tool_call_id.as_deref().unwrap_or("tool");
                out.push_str("<start_of_turn>user\n");
                if first_user && !system_content.is_empty() {
                    out.push_str(system_content);
                    out.push_str("\n\n");
                    first_user = false;
                }
                out.push_str(&format!("Tool result ({tool_id}): {}", msg.content));
                out.push_str("<end_of_turn>\n");
            }
            MessageRole::System => {}
        }
    }
    out.push_str("<start_of_turn>model\n");
    out
}

fn build_plain_prompt(system_content: &str, dialog: &[&ChatMessage]) -> String {
    let mut out = String::new();
    if !system_content.is_empty() {
        out.push_str(system_content);
        out.push_str("\n\n");
    }
    for msg in dialog {
        match msg.role {
            MessageRole::User => {
                out.push_str("User: ");
                out.push_str(&msg.content);
                out.push_str("\n");
            }
            MessageRole::Assistant => {
                out.push_str("Assistant: ");
                out.push_str(&msg.content);
                out.push_str("\n");
            }
            MessageRole::Tool => {
                let tool_id = msg.tool_call_id.as_deref().unwrap_or("tool");
                out.push_str(&format!("Tool ({tool_id}): {}\n", msg.content));
            }
            MessageRole::System => {}
        }
    }
    out.push_str("Assistant: ");
    out
}

fn build_templated_prompt(
    template: &ChatTemplate,
    system_content: &str,
    dialog: &[&ChatMessage],
) -> String {
    let mut out = String::new();
    let mut first_turn = true;
    let mut pending_user = String::new();

    let flush_user = |out: &mut String, first_turn: &mut bool, user: &str| {
        if user.is_empty() {
            return;
        }
        if *first_turn {
            out.push_str(&template.format_first_turn(system_content, user));
            *first_turn = false;
        } else {
            out.push_str(&template.format_continuation(user));
        }
    };

    for msg in dialog {
        match msg.role {
            MessageRole::User => {
                if !pending_user.is_empty() {
                    pending_user.push_str("\n\n");
                }
                pending_user.push_str(&msg.content);
            }
            MessageRole::Assistant => {
                flush_user(&mut out, &mut first_turn, &pending_user);
                pending_user.clear();
                out.push_str(&msg.content);
            }
            MessageRole::Tool => {
                let tool_id = msg.tool_call_id.as_deref().unwrap_or("tool");
                if !pending_user.is_empty() {
                    pending_user.push_str("\n\n");
                }
                pending_user.push_str(&format!("Tool result ({tool_id}): {}", msg.content));
            }
            MessageRole::System => {}
        }
    }

    flush_user(&mut out, &mut first_turn, &pending_user);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemma_prompt_formats_turns() {
        let messages = vec![
            ChatMessage::system("你是助手。"),
            ChatMessage::user("你好"),
        ];
        let prompt = build_prompt(&messages, &PromptStyle::Gemma);
        assert!(prompt.contains("<start_of_turn>user\n你是助手。"));
        assert!(prompt.contains("你好<end_of_turn>"));
        assert!(prompt.ends_with("<start_of_turn>model\n"));
    }

    #[test]
    fn gemma_multi_turn_includes_assistant_history() {
        let messages = vec![
            ChatMessage::user("1+1?"),
            ChatMessage::assistant("2"),
            ChatMessage::user("再乘3?"),
        ];
        let prompt = build_prompt(&messages, &PromptStyle::Gemma);
        assert!(prompt.contains("<start_of_turn>model\n2<end_of_turn>"));
        assert!(prompt.contains("再乘3?<end_of_turn>"));
    }

    #[test]
    fn find_stop_detects_gemma_end_turn() {
        let text = "你好呀<end_of_turn>\n";
        assert_eq!(
            find_stop(text, &PromptStyle::Gemma, &ChatTemplate::None),
            Some(9)
        );
    }
}

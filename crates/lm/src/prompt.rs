use chrono::Local;
use lmrs::tokenizer::Tokenizer;
use lmrs::transformer::ModelType;
use rust_agent_core::{ChatMessage, MessageRole};

/// 将 `ChatMessage` 历史转换为 lm.rs 推理用的 token 序列。
pub fn build_prompt_tokens(
    tokenizer: &mut Tokenizer,
    messages: &[ChatMessage],
    model_type: ModelType,
) -> Vec<u32> {
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

    let mut tokens = Vec::new();

    if model_type == ModelType::LLAMA {
        tokens.extend(llama_system_header(tokenizer));
        if !system_content.is_empty() {
            tokens.extend(tokenizer.encode(
                &system_content,
                false,
                false,
                false,
                model_type,
            ));
        }
        tokens.push(128009);
    }

    if dialog.is_empty() {
        return tokens;
    }

    let last_idx = dialog.len() - 1;

    for (i, msg) in dialog.iter().enumerate() {
        match msg.role {
            MessageRole::User => {
                let mut content = msg.content.clone();
                if model_type != ModelType::LLAMA && i == 0 && !system_content.is_empty() {
                    content = format!("{system_content}\n\n{content}");
                }
                let chat_format = i == last_idx;
                tokens.extend(tokenizer.encode(
                    &content,
                    false,
                    false,
                    chat_format,
                    model_type,
                ));
            }
            MessageRole::Assistant => {
                tokens.extend(tokenizer.encode(
                    &msg.content,
                    false,
                    false,
                    false,
                    model_type,
                ));
            }
            MessageRole::Tool => {
                let tool_id = msg
                    .tool_call_id
                    .as_deref()
                    .unwrap_or("tool");
                let text = format!("Tool result ({tool_id}): {}", msg.content);
                tokens.extend(tokenizer.encode(&text, false, false, false, model_type));
            }
            MessageRole::System => {}
        }
    }

    tokens
}

fn llama_system_header(tokenizer: &mut Tokenizer) -> Vec<u32> {
    let mut tokens = vec![
        128000, 128006, 9125, 128007, 271, 38766, 1303, 33025, 2696, 25, 6790, 220, 2366, 18,
        198, 15724, 2696, 25, 220,
    ];
    let today = Local::now().date_naive().format("%d %b %Y").to_string();
    tokens.extend(tokenizer.encode(
        &today,
        false,
        false,
        false,
        ModelType::LLAMA,
    ));
    tokens.push(271);
    tokens
}

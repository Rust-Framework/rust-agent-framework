#[cfg(feature = "tiktoken")]
use rust_agent_core::MessageRole;

use rust_agent_core::{ChatMessage, ITokenCounter};

use super::estimate::EstimateCounter;

/// 使用 tiktoken-rs 的模型特定 Token 计数器。
///
/// 若 tiktoken 不可用或模型无法识别，则回退到 `EstimateCounter`。
#[cfg(feature = "tiktoken")]
pub struct TiktokenCounter {
    encoding: Option<tiktoken_rs::CoreBPE>,
    fallback: EstimateCounter,
}

#[cfg(feature = "tiktoken")]
impl TiktokenCounter {
    pub fn new(model_id: &str) -> Self {
        let encoding = tiktoken_rs::get_bpe_from_model(model_id)
            .or_else(|_| tiktoken_rs::get_bpe_from_model("gpt-4"))
            .ok();
        if encoding.is_none() {
            tracing::warn!(
                model_id = %model_id,
                "Failed to initialize tiktoken encoding; falling back to estimate counter"
            );
        }
        Self {
            encoding,
            fallback: EstimateCounter::new(),
        }
    }

    pub fn for_model(model_id: impl Into<String>) -> Self {
        Self::new(&model_id.into())
    }
}

#[cfg(feature = "tiktoken")]
impl ITokenCounter for TiktokenCounter {
    fn count_tokens(&self, messages: &[ChatMessage]) -> usize {
        let formatted: Vec<tiktoken_rs::ChatCompletionRequestMessage> = messages
            .iter()
            .map(|msg| tiktoken_rs::ChatCompletionRequestMessage {
                role: match msg.role {
                    MessageRole::System => "system".to_string(),
                    MessageRole::User => "user".to_string(),
                    MessageRole::Assistant => "assistant".to_string(),
                    MessageRole::Tool => "tool".to_string(),
                },
                content: Some(msg.content.clone()),
                name: msg.name.clone(),
                tool_calls: msg.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| tiktoken_rs::ChatCompletionRequestToolCall {
                            id: tc.id.clone(),
                            r#type: "function".to_string(),
                            function: tiktoken_rs::ChatCompletionRequestToolCallFunction {
                                name: tc.name.clone(),
                                arguments: tc.arguments.as_str().unwrap_or("").to_string(),
                            },
                        })
                        .collect()
                }),
                tool_call_id: msg.tool_call_id.clone(),
            })
            .collect();

        tiktoken_rs::num_tokens_from_messages("gpt-4", &formatted)
            .unwrap_or_else(|_| self.fallback.count_tokens(messages))
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        match &self.encoding {
            Some(enc) => enc.encode_with_special_tokens(text).len(),
            None => self.fallback.count_text_tokens(text),
        }
    }
}

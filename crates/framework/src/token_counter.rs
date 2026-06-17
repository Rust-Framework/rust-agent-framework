use rust_agent_core::{ChatMessage, ITokenCounter};

#[cfg(feature = "tiktoken")]
use rust_agent_core::MessageRole;

/// Rough estimate token counter: ~1 token per 4 characters.
///
/// Use this when no model-specific tokenizer is available.
/// The estimate is intentionally conservative (over-counts slightly)
/// to avoid exceeding context windows.
pub struct EstimateCounter {
    /// Characters per token ratio. Default is 4.0.
    pub chars_per_token: f32,
}

impl EstimateCounter {
    pub fn new() -> Self {
        Self {
            chars_per_token: 4.0,
        }
    }

    fn estimate(&self, text: &str) -> usize {
        (text.len() as f32 / self.chars_per_token).ceil() as usize
    }
}

impl Default for EstimateCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl ITokenCounter for EstimateCounter {
    fn count_tokens(&self, messages: &[ChatMessage]) -> usize {
        let mut total = 0;
        for msg in messages {
            // Message formatting overhead: role label + separators ≈ 4 tokens per message
            total += 4;
            total += self.estimate(&msg.content);
            if let Some(ref name) = msg.name {
                total += self.estimate(name) + 1;
            }
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    total += self.estimate(&tc.name) + 3;
                    total += self.estimate(
                        tc.arguments.as_str().unwrap_or(""),
                    );
                }
            }
            if let Some(ref tool_call_id) = msg.tool_call_id {
                total += self.estimate(tool_call_id) + 2;
            }
        }
        // Priming tokens for the assistant response
        total += 3;
        total
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        self.estimate(text)
    }
}

/// Model-specific token counter using tiktoken-rs.
///
/// Falls back to `EstimateCounter` if tiktoken is not available or
/// the model is not recognized.
#[cfg(feature = "tiktoken")]
pub struct TiktokenCounter {
    encoding: tiktoken_rs::CoreBPE,
    fallback: EstimateCounter,
}

#[cfg(feature = "tiktoken")]
impl TiktokenCounter {
    pub fn new(model_id: &str) -> Self {
        let encoding = tiktoken_rs::get_bpe_from_model(model_id)
            .or_else(|_| tiktoken_rs::get_bpe_from_model("gpt-4"))
            .unwrap_or_else(|_| {
                // This shouldn't happen with gpt-4 fallback, but just in case
                panic!("Failed to initialize tiktoken encoding")
            });
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
        // Use tiktoken's chat completion formatting
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
                                arguments: tc
                                    .arguments
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
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
        self.encoding.encode_with_special_tokens(text).len()
    }
}

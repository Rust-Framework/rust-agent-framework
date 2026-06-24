use rust_agent_core::{ChatMessage, ITokenCounter};

/// 粗略估算 Token 计数器：约 1 token 对应 4 个字符。
///
/// 当无可用的模型特定分词器时使用此计数器。
/// 估算有意保守（略高估），以避免超出上下文窗口。
pub struct EstimateCounter {
    /// 每 token 字符数。默认值为 4.0。
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
            total += 4;
            total += self.estimate(&msg.content);
            if let Some(ref name) = msg.name {
                total += self.estimate(name) + 1;
            }
            if let Some(ref tool_calls) = msg.tool_calls {
                for tc in tool_calls {
                    total += self.estimate(&tc.name) + 3;
                    total += self.estimate(tc.arguments.as_str().unwrap_or(""));
                }
            }
            if let Some(ref tool_call_id) = msg.tool_call_id {
                total += self.estimate(tool_call_id) + 2;
            }
        }
        total += 3;
        total
    }

    fn count_text_tokens(&self, text: &str) -> usize {
        self.estimate(text)
    }
}

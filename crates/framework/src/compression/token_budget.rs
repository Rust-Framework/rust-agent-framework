use rust_agent_core::{ChatMessage, ICompressionStrategy, ITokenCounter, MessageRole, Result};

/// Token 预算压缩策略。
///
/// 从最早的消息开始截断，直到总 Token 数符合指定预算。系统消息始终保留。
///
/// 此策略遵循 MAF 的 `ContextWindowCompactionStrategy` 方法：
/// 1. 首先尝试移除最早的非系统消息以符合预算
/// 2. 始终保留系统消息和最近的消息
pub struct TokenBudgetStrategy {
    /// 开始移除工具结果组的预算阈值比例。
    /// 默认：0.5（达到 50% 预算时开始移除）。
    pub tool_result_eviction_threshold: f64,
}

impl TokenBudgetStrategy {
    pub fn new() -> Self {
        Self {
            tool_result_eviction_threshold: 0.5,
        }
    }

    /// Create with custom tool result eviction threshold.
    pub fn with_eviction_threshold(mut self, threshold: f64) -> Self {
        self.tool_result_eviction_threshold = threshold.clamp(0.0, 1.0);
        self
    }
}

impl Default for TokenBudgetStrategy {
    fn default() -> Self {
        Self::new()
    }
}

impl ICompressionStrategy for TokenBudgetStrategy {
    fn name(&self) -> &str {
        "TokenBudget"
    }

    fn compress(
        &self,
        messages: Vec<ChatMessage>,
        budget: usize,
        counter: &dyn ITokenCounter,
    ) -> Result<Vec<ChatMessage>> {
        let current_tokens = counter.count_tokens(&messages);

        // If already within budget, return as-is
        if current_tokens <= budget {
            return Ok(messages);
        }

        // Separate system messages from the rest
        let mut system_msgs: Vec<ChatMessage> = Vec::new();
        let mut other_msgs: Vec<ChatMessage> = Vec::new();

        for msg in messages {
            if msg.role == MessageRole::System {
                system_msgs.push(msg);
            } else {
                other_msgs.push(msg);
            }
        }

        // Phase 1: Tool result eviction — fold old tool call groups into summaries
        // when tokens exceed the eviction threshold
        let eviction_budget = (budget as f64 * self.tool_result_eviction_threshold) as usize;
        if current_tokens > eviction_budget {
            other_msgs = self.evict_tool_results(other_msgs);
        }

        // Phase 2: Truncation — remove oldest non-system messages until within budget
        let mut result = system_msgs.clone();
        result.extend(other_msgs.clone());

        while counter.count_tokens(&result) > budget && other_msgs.len() > 1 {
            // Remove the oldest non-system message
            other_msgs.remove(0);
            result = system_msgs.clone();
            result.extend(other_msgs.clone());
        }

        let final_tokens = counter.count_tokens(&result);
        tracing::info!(
            strategy = self.name(),
            before_tokens = current_tokens,
            after_tokens = final_tokens,
            budget = budget,
            kept_messages = result.len(),
            "Token budget compression applied"
        );

        Ok(result)
    }
}

impl TokenBudgetStrategy {
    /// Evict tool result groups: replace old assistant+tool message pairs
    /// with a compact summary, preserving the tool call structure.
    fn evict_tool_results(&self, messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
        let mut result = Vec::with_capacity(messages.len());
        let mut i = 0;

        while i < messages.len() {
            let msg = &messages[i];

            // Check if this is an assistant message with tool_calls
            if msg.role == MessageRole::Assistant {
                if let Some(ref tool_calls) = msg.tool_calls {
                    if !tool_calls.is_empty() {
                        // Collect the assistant message and following tool result messages
                        let mut group = vec![msg.clone()];
                        let tool_count = tool_calls.len();
                        let mut j = i + 1;
                        let mut tool_results_collected = 0;

                        while j < messages.len() && tool_results_collected < tool_count {
                            if messages[j].role == MessageRole::Tool {
                                group.push(messages[j].clone());
                                tool_results_collected += 1;
                            } else {
                                break;
                            }
                            j += 1;
                        }

                        // Replace the group with a compact summary
                        let summary = format!(
                            "[Earlier tool calls: {} call(s) were made and completed]",
                            tool_calls.len()
                        );
                        result.push(ChatMessage::assistant(summary));
                        i = j;
                        continue;
                    }
                }
            }

            result.push(msg.clone());
            i += 1;
        }

        result
    }
}

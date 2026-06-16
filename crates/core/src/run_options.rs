use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::chat_client::ChatClientRunOptions;
use crate::tool::ToolApprovalResponse;

/// Options passed to `IAgent::run()`, following MAF's RunOptions pattern.
///
/// Allows callers to override per-call behaviour without mutating
/// the agent's persistent configuration. Fields are `Option`-al —
/// `None` means "use the agent's default".
///
/// MAF reference: `AgentRunOptions` in Microsoft Agent Framework.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRunOptions {
    /// Override the system instructions for this run only.
    pub instructions: Option<String>,
    /// Override max_tokens for this run only.
    pub max_tokens: Option<u32>,
    /// Override temperature for this run only.
    pub temperature: Option<f32>,
    /// Override top_p for this run only.
    pub top_p: Option<f32>,
    /// Override stop sequences for this run only.
    pub stop: Option<Vec<String>>,
    /// Extra JSON fields merged into the chat completion request body
    /// for this run only (e.g. DeepSeek thinking config).
    pub extra_body: HashMap<String, serde_json::Value>,
    /// Arbitrary properties passed through to the agent run context.
    pub properties: HashMap<String, serde_json::Value>,
    /// Allow parallel tool calls. When `Some(true)`, the LLM may emit multiple
    /// tool calls in a single response. Maps to OpenAI's `parallel_tool_calls` parameter.
    pub parallel_tool_calls: Option<bool>,
    /// Tool approval responses for resuming after `FinishReason::AwaitingApproval`.
    /// Caller fills this with user decisions before calling `run()` again.
    /// The session already holds the assistant(tool_calls) message from the
    /// paused run, so no messages need to be passed.
    pub tool_approval_responses: Vec<ToolApprovalResponse>,
    /// Cancel flag. The caller holds a clone and sets it to `true` to interrupt
    /// the agent at the next tool-loop iteration. Zero external dependencies.
    #[serde(skip)]
    pub cancelled: Option<Arc<AtomicBool>>,
}

impl AgentRunOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    pub fn with_extra_body(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.extra_body.insert(key.into(), value);
        self
    }

    pub fn with_properties(
        mut self,
        iter: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) -> Self {
        self.properties.extend(iter);
        self
    }

    /// Enable or disable DeepSeek thinking (reasoning) mode for this run.
    ///
    /// When enabled, the model outputs `reasoning_content` in stream deltas
    /// before the final `content`.
    pub fn with_thinking(mut self, enabled: bool) -> Self {
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        self.extra_body.insert(
            "thinking".to_string(),
            serde_json::json!({ "type": thinking_type }),
        );
        self
    }

    /// Set tool approval responses for resuming after an approval pause.
    pub fn with_tool_approval_responses(
        mut self,
        responses: Vec<ToolApprovalResponse>,
    ) -> Self {
        self.tool_approval_responses = responses;
        self
    }

    /// Set the cancellation flag for this run.
    pub fn with_cancelled(mut self, flag: Arc<AtomicBool>) -> Self {
        self.cancelled = Some(flag);
        self
    }

    /// Set the reasoning effort level for this run.
    ///
    /// Maps to `reasoning_effort: "high"/"max"` in the request body.
    pub fn with_reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.extra_body.insert(
            "reasoning_effort".to_string(),
            serde_json::to_value(effort).unwrap(),
        );
        self
    }

    /// Convert to `ChatClientRunOptions` for passing to `IChatClient::run()`.
    ///
    /// Agent-level fields (like `instructions`) are handled by the agent
    /// and not forwarded to the chat client.
    pub fn to_chat_client_run_options(&self) -> ChatClientRunOptions {
        ChatClientRunOptions {
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            stop: self.stop.clone(),
            extra_body: self.extra_body.clone(),
            tools: Vec::new(), // tools are injected by the agent, not from options
            parallel_tool_calls: self.parallel_tool_calls,
            provider_tools: Vec::new(), // injected on_invoking(), not from AgentRunOptions
            tool_approval_responses: self.tool_approval_responses.clone(),
            cancelled: self.cancelled.clone(),
        }
    }
}

/// Reasoning effort level for models that support it (e.g. DeepSeek).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    High,
    Max,
}

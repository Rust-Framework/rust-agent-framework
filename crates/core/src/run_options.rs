use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::chat_client::ChatClientRunOptions;
use crate::tool::ToolApprovalResponse;

/// 传递给 `IAgent::run()` 的选项，遵循 MAF 的 RunOptions 模式。
///
/// 允许调用方覆盖每次调用的行为，而不修改智能体的持久配置。
/// 字段均为 `Option` 类型——`None` 表示"使用智能体默认值"。
///
/// MAF 参考：Microsoft Agent Framework 中的 `AgentRunOptions`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRunOptions {
    /// 仅覆盖本次运行的系统指令。
    pub instructions: Option<String>,
    /// 仅覆盖本次运行的 max_tokens。
    pub max_tokens: Option<u32>,
    /// 仅覆盖本次运行的 temperature。
    pub temperature: Option<f32>,
    /// 仅覆盖本次运行的 top_p。
    pub top_p: Option<f32>,
    /// 仅覆盖本次运行的 stop 序列。
    pub stop: Option<Vec<String>>,
    /// 仅覆盖本次运行中合并到补全请求体的额外 JSON 字段
    /// （如 DeepSeek 思考配置）。
    pub extra_body: HashMap<String, serde_json::Value>,
    /// 传递到智能体运行上下文的任意属性。
    pub properties: HashMap<String, serde_json::Value>,
    /// 允许并行工具调用。当为 `Some(true)` 时，LLM 可在单次响应中发出多个
    /// 工具调用。映射到 OpenAI 的 `parallel_tool_calls` 参数。
    pub parallel_tool_calls: Option<bool>,
    /// 在 `FinishReason::AwaitingApproval` 后恢复运行的工具审批响应。
    /// 调用方在再次调用 `run()` 前填入用户决策。
    /// 会话已持有暂停运行的 assistant(tool_calls) 消息，因此无需传递消息。
    pub tool_approval_responses: Vec<ToolApprovalResponse>,
    /// 取消标志。调用方持有克隆并将其设为 `true` 以在下一个工具循环
    /// 迭代时中断智能体。零外部依赖。
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

    /// 启用或禁用本次运行的 DeepSeek 思考（推理）模式。
    ///
    /// 启用后，模型在流式增量中先输出 `reasoning_content`，
    /// 再输出最终的 `content`。
    pub fn with_thinking(mut self, enabled: bool) -> Self {
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        self.extra_body.insert(
            "thinking".to_string(),
            serde_json::json!({ "type": thinking_type }),
        );
        self
    }

    /// 控制 LLM 是否可以在一次响应中发出多个工具调用。
    pub fn with_parallel_tool_calls(mut self, enabled: bool) -> Self {
        self.parallel_tool_calls = Some(enabled);
        self
    }

    /// 设置审批暂停后恢复运行的工具审批响应。
    pub fn with_tool_approval_responses(
        mut self,
        responses: Vec<ToolApprovalResponse>,
    ) -> Self {
        self.tool_approval_responses = responses;
        self
    }

    /// 设置本次运行的取消标志。
    pub fn with_cancelled(mut self, flag: Arc<AtomicBool>) -> Self {
        self.cancelled = Some(flag);
        self
    }

    /// 设置本次运行的推理努力级别。
    ///
    /// 映射到请求体中的 `reasoning_effort: "high"/"max"`。
    pub fn with_reasoning_effort(mut self, effort: ReasoningEffort) -> Self {
        self.extra_body.insert(
            "reasoning_effort".to_string(),
            serde_json::to_value(effort).unwrap(),
        );
        self
    }

    /// 转换为 `ChatClientRunOptions` 以传递给 `IChatClient::run()`。
    ///
    /// 智能体级别的字段（如 `instructions`）由智能体处理，不会转发给聊天客户端。
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

/// 支持该功能的模型（如 DeepSeek）的推理努力级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    High,
    Max,
}

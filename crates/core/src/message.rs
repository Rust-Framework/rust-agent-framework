use crate::types::{AgentId, FinishReason, ResponseMetadata, ToolCall, Usage};
use serde::{Deserialize, Serialize};

/// 消息来源标记，用于追踪消息的起源。
///
/// `InMemoryHistoryProvider` 使用此标记在持久化时过滤消息，
/// 避免重复存储历史消息。
/// 参考自 MAF 的 `AgentRequestMessageSourceType`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageSource {
    /// 外部用户输入
    External,
    /// 从聊天历史加载
    ChatHistory,
    /// 由 ContextProvider 注入
    ContextProvider,
    /// 工具执行结果
    ToolResult,
}

/// 消息作者角色，遵循 MAF 的统一 ChatMessage 模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// 扩展的 ChatMessage — 包含 tool_calls 和 tool_call_id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tool_call_id: Option<String>,
    /// Message source marker for tracking origin.
    /// Used to prevent duplicate persistence of history messages.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<MessageSource>,
}

impl ChatMessage {
    /// 创建系统角色消息
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            source: None,
        }
    }

    /// 创建用户角色消息
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            source: None,
        }
    }

    /// 创建助手角色消息
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            source: None,
        }
    }

    /// 创建包含工具调用的助手角色消息
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            name: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            source: None,
        }
    }

    /// 创建工具角色消息（工具执行结果）
    pub fn tool(content: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            source: None,
        }
    }
}

/// 从内容/事件变体中获取 ResponseMetadata 的 trait
pub trait HasMeta {
    /// 获取响应元数据引用
    fn meta(&self) -> &ResponseMetadata;
}

// === Content types ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    pub meta: ResponseMetadata,
    pub delta: String,
}
impl HasMeta for TextContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningContent {
    pub meta: ResponseMetadata,
    pub delta: String,
}
impl HasMeta for ReasoningContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UriContent {
    pub meta: ResponseMetadata,
    pub uri: String,
    pub label: Option<String>,
}
impl HasMeta for UriContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// ── 工具调用生命周期（5 阶段）────────────────────────────────────
///
/// 工具调用从 LLM 到执行完成经过完整的生命周期，每个阶段对应一个 Content 类型：
///
/// ```text
/// ToolCallStart → ToolCallArgs(×N) → ToolCallEnd → ToolCalling → ToolCalled
///     ①开始         ②参数流式到达       ③参数完毕     ④完整调用     ⑤执行结果
/// ```
///
/// - ①~③ 是**流式阶段**，在 SSE 数据到达时实时发出，消费方可据此展示进度
/// - ④ 是**汇总阶段**，在流结束时一次性发出，携带完整解析后的参数结构体
/// - ⑤ 是**执行阶段**，由 FunctionInvokingChatClient 管道装饰器执行后发出

/// ④ 完整工具调用 — 流式参数已收集完毕，arguments 被解析为 `serde_json::Value`。
///
/// 在流结束时（`FinishReason::ToolCalls`）由 `AgentResponseConverter::flush_tool_calls()`
/// 生成，是 ①~③ 三个阶段收集到的参数的汇总体，**可直接传给工具执行**。
///
/// 生命周期位置：`ToolCallEnd → ToolCalling → ToolCalled`
/// 对应 MAF .NET 的 `FunctionCallContent`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallingContent {
    pub meta: ResponseMetadata,
    pub call_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
impl HasMeta for ToolCallingContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// ① 工具调用开始 — LLM 开始生成一个新的工具调用。
///
/// 在参数流到达之前发出，消费方可据此展示"正在准备调用 XX 工具"。
/// 每个工具调用只产生**一次**此事件。
///
/// 生命周期位置：`ToolCallStart → ToolCallArgs → ToolCallEnd → ToolCalling → ToolCalled`
/// 对应 MAF .NET AGUI 的 `ToolCallStartEvent`。
///
/// 与 [ToolCallingContent] 的区别：`ToolCallStartContent` 在流式接收阶段实时发出、
/// 不携带参数；`ToolCallingContent` 在流结束后发出、携带完整的已解析参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallStartContent {
    pub meta: ResponseMetadata,
    pub call_id: String,
    pub name: String,
}
impl HasMeta for ToolCallStartContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// ② 工具调用参数增量 — LLM 正在流式输出工具调用的 JSON 参数。
///
/// 每次携带一个 `args_delta` 片段，需由消费方拼接。在 `ToolCallStart` 之后、
/// `ToolCallEnd` 之前可能产生**多次**。
///
/// 生命周期位置：`ToolCallStart → ToolCallArgs(×N) → ToolCallEnd → ToolCalling → ToolCalled`
/// 对应 MAF .NET AGUI 的 `ToolCallArgsEvent`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallArgsContent {
    pub meta: ResponseMetadata,
    pub call_id: String,
    pub args_delta: String,
}
impl HasMeta for ToolCallArgsContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// ③ 工具调用参数完毕 — LLM 已完成该工具调用的所有参数输出。
///
/// 此时参数尚未被解析为结构化 JSON（④ 阶段才做），但消费方可据此标记
/// "参数接收完成，准备执行"。
///
/// 生命周期位置：`ToolCallStart → ToolCallArgs → ToolCallEnd → ToolCalling → ToolCalled`
/// 对应 MAF .NET AGUI 的 `ToolCallEndEvent`。
///
/// 与 [ToolCalledContent] 的区别：`ToolCallEndContent` 在**执行前**发出，仅表示参数
/// 流式传输完毕；`ToolCalledContent` 在**执行后**发出，携带 `result` 或 `error`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEndContent {
    pub meta: ResponseMetadata,
    pub call_id: String,
}
impl HasMeta for ToolCallEndContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// ⑤ 工具调用执行结果 — 工具已执行完毕，携带返回值或错误信息。
///
/// 由 TokenLoopAgent / ToolMiddleware 在执行工具后发出。
/// `result` 和 `error` 互斥：成功时 `result` 有值、`error` 为 `None`；失败时反之。
///
/// 生命周期位置：`... → ToolCalling → ToolCalled`
/// 对应 MAF .NET 的 `FunctionResultContent`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCalledContent {
    pub meta: ResponseMetadata,
    pub call_id: String,
    pub result: Option<String>,
    pub error: Option<String>,
}
impl HasMeta for ToolCalledContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// ②b 工具调用参数已解析 — 流式 JSON 解析器检测到一个参数键值对已完整。
///
/// 在 `ToolCallArgs` 流中，由 `StreamingArgsParser` 实时解析 JSON 后发出。
/// 例如参数 `{"text": "hello"}` 在引号闭合时立即发出 `ToolCallArgsParsed{name:"text", value:"hello"}`，
/// 无需等待整个 JSON 对象完成。
///
/// 用途：UI 可据此展示已完成的参数，如文件写工具已探测到文件路径后再展示写入内容进度。
///
/// 生命周期位置：`ToolCallStart → ToolCallArgs(xN, 同时ToolCallArgsParsed) → ToolCallEnd → ...`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallArgsParsedContent {
    pub meta: ResponseMetadata,
    pub call_id: String,
    pub name: String,
    pub value: serde_json::Value,
}
impl HasMeta for ToolCallArgsParsedContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// ②c 工具调用参数接收中 — LLM 正在流式输出该参数的字符串值。
///
/// 由 `StreamingArgsParser` 在字符串值未闭合时持续发出，携带已接收的部分内容。
/// `received` 是累积字节数（从开引号计），`value` 是当前已收到的内容片段。
///
/// 用途：用户写入长文本/代码/报告时，可实时展示写入进度和部分内容预览。
///
/// 生命周期位置：在 `ToolCallArgsParsed` 之前，字符串值接收过程中持续发出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallArgsProgressContent {
    pub meta: ResponseMetadata,
    pub call_id: String,
    pub name: String,
    pub received: usize,
    pub value: serde_json::Value,
}
impl HasMeta for ToolCallArgsProgressContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageContent {
    pub meta: ResponseMetadata,
    pub usage: Usage,
}
impl HasMeta for UsageContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContent {
    pub meta: ResponseMetadata,
    pub error_code: String,
    pub message: String,
}
impl HasMeta for ErrorContent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

/// Content 枚举 — 12 个变体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Content {
    Text(TextContent),
    Reasoning(ReasoningContent),
    Uri(UriContent),
    ToolCallStart(ToolCallStartContent),
    ToolCallArgs(ToolCallArgsContent),
    ToolCallArgsParsed(ToolCallArgsParsedContent),
    ToolCallArgsProgress(ToolCallArgsProgressContent),
    ToolCallEnd(ToolCallEndContent),
    ToolCalling(ToolCallingContent),
    ToolCalled(ToolCalledContent),
    Usage(UsageContent),
    Error(ErrorContent),
}

// === Event types ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorInvokingEvent {
    pub meta: ResponseMetadata,
    pub executor_id: String,
    pub executor_type: String,
    pub input_message_count: usize,
}
impl HasMeta for ExecutorInvokingEvent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorInvokedEvent {
    pub meta: ResponseMetadata,
    pub executor_id: String,
    pub duration_ms: u64,
    pub output_content_count: usize,
}
impl HasMeta for ExecutorInvokedEvent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomEvent {
    pub meta: ResponseMetadata,
    pub event_type: String,
    pub payload: serde_json::Value,
}
impl HasMeta for CustomEvent {
    fn meta(&self) -> &ResponseMetadata {
        &self.meta
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    ExecutorInvoking(ExecutorInvokingEvent),
    ExecutorInvoked(ExecutorInvokedEvent),
    Custom(CustomEvent),
}

// === Public API: AgentResponseResult ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponseResult {
    pub id: Option<String>,
    pub model: Option<String>,
    pub finish_reason: Option<FinishReason>,
    pub contents: Vec<Content>,
    pub events: Vec<Event>,
}

// === Internal type: AgentResponseUpdate ===
// This is the SSE-parse-level type. Marked pub because client crate needs it,
// but documented as internal.

#[derive(Debug, Clone)]
pub enum AgentResponseUpdate {
    TextDelta { delta: String },
    ReasoningDelta { delta: String },
    /// Legacy flat tool call delta — used by the transport layer when the SSE
    /// format doesn't separate start/args/end events. The converter will
    /// decompose this into ToolCallStart / ToolCallArgs / ToolCallEnd.
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    /// A new tool call has started. Includes the unique call ID and tool name.
    ToolCallStart { id: String, name: String },
    /// Streaming arguments delta for an in-progress tool call.
    ToolCallArgs { id: String, args_delta: String },
    /// A tool call's arguments are complete.
    ToolCallEnd { id: String },
    /// A tool call has been executed, carrying the result or error.
    ToolCalled { id: String, result: Option<String>, error: Option<String> },
    /// A tool call requires human approval before execution.
    /// Corresponds to MAF's `FunctionApprovalRequestContent`.
    ToolApprovalRequest {
        call_id: String,
        name: String,
        arguments: serde_json::Value,
        description: String,
    },
    Usage { usage: Usage },
    Finish {
        finish_reason: FinishReason,
        usage: Option<Usage>,
    },
    Error { message: String },
    ResponseMetadata { id: Option<String>, model: Option<String> },
}

// === Extended AgentResponse ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub text: String,
    pub reasoning_text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    /// Tool-role messages for this turn's tool results (paired with `tool_calls`).
    #[serde(default)]
    pub tool_messages: Vec<ChatMessage>,
    /// Full multi-round transcript for this agent run (user + assistant/tool chain).
    /// Used by MemoryAgent selective context projection — excludes MainAgent system.
    #[serde(default)]
    pub turn_transcript: Vec<ChatMessage>,
    pub finish_reason: Option<FinishReason>,
    pub usage: Option<Usage>,
    pub source_agent_id: Option<AgentId>,
}

impl AgentResponse {
    /// 从纯文本创建 Agent 响应（无工具调用）
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            id: None,
            model: None,
            text: text.into(),
            reasoning_text: None,
            tool_calls: Vec::new(),
            tool_messages: Vec::new(),
            turn_transcript: Vec::new(),
            finish_reason: None,
            usage: None,
            source_agent_id: None,
        }
    }
}

use crate::types::{AgentId, FinishReason, ResponseMetadata, ToolCall, Usage};
use serde::{Deserialize, Serialize};

/// Role of a message author, following MAF's unified ChatMessage model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Extended ChatMessage — now includes tool_calls and tool_call_id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            name: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    pub fn tool(content: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    #[deprecated(note = "use ChatMessage::tool(content, tool_call_id) instead")]
    pub fn tool_with_name(content: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            name: Some(name.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

/// Trait to get ResponseMetadata from content/event variants
pub trait HasMeta {
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

/// Content enum — 7 variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Content {
    Text(TextContent),
    Reasoning(ReasoningContent),
    Uri(UriContent),
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
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
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
    pub finish_reason: Option<FinishReason>,
    pub usage: Option<Usage>,
    pub source_agent_id: Option<AgentId>,
}

impl AgentResponse {
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            id: None,
            model: None,
            text: text.into(),
            reasoning_text: None,
            tool_calls: Vec::new(),
            finish_reason: None,
            usage: None,
            source_agent_id: None,
        }
    }
}

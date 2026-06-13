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

/// Unified message type following MAF's ChatMessage abstraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: MessageRole::System, content: content.into(), name: None }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: MessageRole::User, content: content.into(), name: None }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: MessageRole::Assistant, content: content.into(), name: None }
    }

    pub fn tool(content: impl Into<String>, name: impl Into<String>) -> Self {
        Self { role: MessageRole::Tool, content: content.into(), name: Some(name.into()) }
    }
}

/// Streaming chunk from a chat client, following MAF's AgentResponseUpdate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStreamChunk {
    pub text_delta: Option<String>,
    pub tool_call_delta: Option<crate::types::ToolCallDelta>,
}

/// Streaming chunk from an agent, extending ChatStreamChunk with agent context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStreamChunk {
    pub text_delta: Option<String>,
    pub tool_call_delta: Option<crate::types::ToolCallDelta>,
    pub source_agent_id: Option<crate::AgentId>,
}

/// Aggregated agent response, collected from a stream of AgentStreamChunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub text: String,
    pub tool_calls: Vec<crate::types::ToolCall>,
    pub source_agent_id: Option<crate::AgentId>,
}

impl AgentResponse {
    pub fn from_text(text: impl Into<String>) -> Self {
        Self { text: text.into(), tool_calls: Vec::new(), source_agent_id: None }
    }
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A declarative workflow action. Aligns with MAF Declarative Workflows action kinds.
///
/// Actions are the building blocks of declarative workflows. Each action performs
/// a specific operation, and actions are executed sequentially in the order they
/// appear. The complete reference covers 25+ action kinds across variable
/// management, control flow, output, agent/tool invocation, HTTP/MCP integration,
/// human-in-the-loop, and workflow control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ActionDecl {
    // ── Variable Management ──

    /// Sets a variable to a specified value. Supports PowerFx expressions with `=`.
    SetVariable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "displayName")]
        display_name: Option<String>,
        /// Variable path (e.g., `Local.name`).
        variable: String,
        /// Value to set (literal or `=expression`).
        value: serde_json::Value,
    },

    /// Sets multiple variables in a single action.
    SetMultipleVariables {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Map of variable paths to values.
        variables: HashMap<String, serde_json::Value>,
    },

    /// Sets a text variable to a specified string value.
    SetTextVariable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        variable: String,
        value: String,
    },

    /// Clears a variable's value.
    ResetVariable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        variable: String,
    },

    /// Resets all variables in the current context.
    ClearAllVariables {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },

    /// Extracts or converts data into a usable format.
    ParseValue {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        source: String,
        variable: String,
    },

    /// Modifies data in a structured table format.
    EditTableV2 {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        table: String,
        /// Operation: "add", "update", "delete".
        operation: String,
        /// Row data for the operation.
        row: HashMap<String, serde_json::Value>,
    },

    // ── Control Flow ──

    /// Executes actions conditionally based on a PowerFx expression.
    If {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// PowerFx expression that evaluates to true/false.
        condition: String,
        /// Actions to execute if condition is true.
        #[serde(rename = "then")]
        then_actions: Vec<ActionDecl>,
        /// Actions to execute if condition is false.
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "else")]
        else_actions: Option<Vec<ActionDecl>>,
    },

    /// Evaluates multiple conditions like a switch/case statement (first match wins).
    ConditionGroup {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// List of condition/actions pairs.
        conditions: Vec<ConditionBranch>,
        /// Actions if no condition matches.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        else_actions: Option<Vec<ActionDecl>>,
    },

    /// Iterates over a collection.
    Foreach {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// Expression returning a collection.
        source: String,
        /// Variable name for current item (default: "item").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_name: Option<String>,
        /// Variable name for current index (default: "index").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index_name: Option<String>,
        /// Actions to execute for each item.
        actions: Vec<ActionDecl>,
    },

    /// Exits the current loop immediately.
    BreakLoop,

    /// Skips to the next iteration of the loop.
    ContinueLoop,

    /// Jumps to a specific action by its ID.
    GotoAction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// ID of the action to jump to.
        #[serde(rename = "actionId")]
        action_id: String,
    },

    // ── Output ──

    /// Sends a message to the user.
    SendActivity {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        activity: SendActivityPayload,
    },

    // ── Agent Invocation ──

    /// Invokes a registered agent.
    InvokeAgent {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        agent: AgentRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<AgentInput>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<AgentOutput>,
    },

    // ── Tool Invocation ──

    /// Invokes a function tool directly without going through an AI agent.
    InvokeFunctionTool {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(rename = "functionName")]
        function_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation_id: Option<String>,
        #[serde(default)]
        require_approval: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments: Option<HashMap<String, serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<ToolOutput>,
    },

    /// Invokes a tool on an MCP (Model Context Protocol) server.
    InvokeMcpTool {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(rename = "serverUrl")]
        server_url: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conversation_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arguments: Option<HashMap<String, serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<ToolOutput>,
    },

    /// Sends an HTTP request.
    HttpRequestAction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        url: String,
        #[serde(default = "default_http_method")]
        method: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<HashMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query_parameters: Option<HashMap<String, serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<HttpBody>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_headers: Option<String>,
    },

    // ── Human-in-the-Loop ──

    /// Asks the user a question and stores the response.
    Question {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        question: QuestionPayload,
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    /// Requests input from an external system or process.
    RequestExternalInput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        prompt: QuestionPayload,
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    // ── Workflow Control ──

    /// Terminates the workflow execution.
    EndWorkflow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },

    /// Ends the current conversation.
    EndConversation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },

    /// Creates a new conversation context.
    CreateConversation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(rename = "conversationId")]
        conversation_id: String,
    },

    // ── Conversation (C# only) ──

    /// Adds a message to a conversation thread.
    AddConversationMessage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        message: MessagePayload,
    },
}

fn default_http_method() -> String {
    "GET".into()
}

// ── Auxiliary Types ──

/// A branch within a `ConditionGroup` action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionBranch {
    /// PowerFx expression that evaluates to true/false.
    pub condition: String,
    /// Optional branch identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Actions to execute if this condition matches.
    pub actions: Vec<ActionDecl>,
}

/// Reference to a registered agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRef {
    /// Agent name (registered identifier).
    pub name: String,
}

/// Payload for `SendActivity` action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendActivityPayload {
    /// Message text (literal or `=expression`).
    pub text: serde_json::Value,
}

/// A question or prompt for human-in-the-loop actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionPayload {
    pub text: String,
}

/// Input configuration for agent invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInput {
    /// Messages to send to the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    /// Additional arguments for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, serde_json::Value>>,
    /// External loop configuration (continues until condition is met).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_loop: Option<ExternalLoop>,
}

/// External loop configuration for agent invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLoop {
    /// PowerFx condition to continue the loop.
    pub when: String,
}

/// Output configuration for agent invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    /// Path to store agent response object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_object: Option<String>,
    /// Path to store conversation messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    /// Automatically send response to user.
    #[serde(default)]
    pub auto_send: Option<bool>,
}

/// Output configuration for tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Path to store tool result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Path to store result messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    /// Automatically send result to user.
    #[serde(default)]
    pub auto_send: Option<bool>,
}

/// HTTP request body variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HttpBody {
    /// JSON body.
    Json { value: serde_json::Value },
    /// Raw string body.
    Raw { value: String },
    /// No body.
    None,
}

/// A conversation message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    /// Message role (e.g., "user", "assistant", "system").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Message content.
    pub content: String,
}

impl ActionDecl {
    /// Get the action kind string (MAF-compatible).
    pub fn kind_str(&self) -> &'static str {
        match self {
            ActionDecl::SetVariable { .. } => "SetVariable",
            ActionDecl::SetMultipleVariables { .. } => "SetMultipleVariables",
            ActionDecl::SetTextVariable { .. } => "SetTextVariable",
            ActionDecl::ResetVariable { .. } => "ResetVariable",
            ActionDecl::ClearAllVariables { .. } => "ClearAllVariables",
            ActionDecl::ParseValue { .. } => "ParseValue",
            ActionDecl::EditTableV2 { .. } => "EditTableV2",
            ActionDecl::If { .. } => "If",
            ActionDecl::ConditionGroup { .. } => "ConditionGroup",
            ActionDecl::Foreach { .. } => "Foreach",
            ActionDecl::BreakLoop => "BreakLoop",
            ActionDecl::ContinueLoop => "ContinueLoop",
            ActionDecl::GotoAction { .. } => "GotoAction",
            ActionDecl::SendActivity { .. } => "SendActivity",
            ActionDecl::InvokeAgent { .. } => "InvokeAgent",
            ActionDecl::InvokeFunctionTool { .. } => "InvokeFunctionTool",
            ActionDecl::InvokeMcpTool { .. } => "InvokeMcpTool",
            ActionDecl::HttpRequestAction { .. } => "HttpRequestAction",
            ActionDecl::Question { .. } => "Question",
            ActionDecl::RequestExternalInput { .. } => "RequestExternalInput",
            ActionDecl::EndWorkflow { .. } => "EndWorkflow",
            ActionDecl::EndConversation { .. } => "EndConversation",
            ActionDecl::CreateConversation { .. } => "CreateConversation",
            ActionDecl::AddConversationMessage { .. } => "AddConversationMessage",
        }
    }

    /// Get the action ID, if present.
    pub fn action_id(&self) -> Option<&str> {
        match self {
            ActionDecl::SetVariable { id, .. }
            | ActionDecl::SetMultipleVariables { id, .. }
            | ActionDecl::SetTextVariable { id, .. }
            | ActionDecl::ResetVariable { id, .. }
            | ActionDecl::ClearAllVariables { id, .. }
            | ActionDecl::ParseValue { id, .. }
            | ActionDecl::EditTableV2 { id, .. }
            | ActionDecl::If { id, .. }
            | ActionDecl::ConditionGroup { id, .. }
            | ActionDecl::Foreach { id, .. }
            | ActionDecl::GotoAction { id, .. }
            | ActionDecl::SendActivity { id, .. }
            | ActionDecl::InvokeAgent { id, .. }
            | ActionDecl::InvokeFunctionTool { id, .. }
            | ActionDecl::InvokeMcpTool { id, .. }
            | ActionDecl::HttpRequestAction { id, .. }
            | ActionDecl::Question { id, .. }
            | ActionDecl::RequestExternalInput { id, .. }
            | ActionDecl::EndWorkflow { id, .. }
            | ActionDecl::EndConversation { id, .. }
            | ActionDecl::CreateConversation { id, .. }
            | ActionDecl::AddConversationMessage { id, .. } => id.as_deref(),
            ActionDecl::BreakLoop | ActionDecl::ContinueLoop => None,
        }
    }
}

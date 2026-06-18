use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 声明式工作流动作，与 MAF Declarative Workflows 动作类型对齐。
///
/// 动作是声明式工作流的基本构建块，每个动作执行特定操作，
/// 按出现顺序顺序执行。完整参考涵盖 25+ 种动作类型，涵盖
/// 变量管理、控制流、输出、Agent/工具调用、HTTP/MCP 集成、
/// 人机交互和工作流控制。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ActionDecl {
    // ── 变量管理 ──

    /// 将变量设置为指定值，支持带 `=` 的 PowerFx 表达式。
    SetVariable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "displayName")]
        display_name: Option<String>,
        /// 变量路径（例如 `Local.name`）。
        variable: String,
        /// 要设置的值（字面量或 `=expression`）。
        value: serde_json::Value,
    },

    /// 在单个动作中设置多个变量。
    SetMultipleVariables {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// 变量路径到值的映射。
        variables: HashMap<String, serde_json::Value>,
    },

    /// 将文本变量设置为指定的字符串值。
    SetTextVariable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        variable: String,
        value: String,
    },

    /// 清除变量的值。
    ResetVariable {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        variable: String,
    },

    /// 重置当前上下文中的所有变量。
    ClearAllVariables {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },

    /// 提取或转换数据为可用格式。
    ParseValue {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        source: String,
        variable: String,
    },

    /// 修改结构化表格格式的数据。
    EditTableV2 {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        table: String,
        /// 操作："add"、"update"、"delete"。
        operation: String,
        /// 操作的行数据。
        row: HashMap<String, serde_json::Value>,
    },

    // ── 控制流 ──

    /// 基于 PowerFx 表达式条件执行动作。
    If {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// 求值为 true/false 的 PowerFx 表达式。
        condition: String,
        /// 条件为 true 时执行的动作。
        #[serde(rename = "then")]
        then_actions: Vec<ActionDecl>,
        /// 条件为 false 时执行的动作。
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "else")]
        else_actions: Option<Vec<ActionDecl>>,
    },

    /// 像 switch/case 语句一样评估多个条件（首个匹配生效）。
    ConditionGroup {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// 条件/动作对的列表。
        conditions: Vec<ConditionBranch>,
        /// 无条件匹配时执行的动作。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        else_actions: Option<Vec<ActionDecl>>,
    },

    /// 遍历集合。
    Foreach {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// 返回集合的表达式。
        source: String,
        /// 当前项的变量名称（默认："item"）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_name: Option<String>,
        /// 当前索引的变量名称（默认："index"）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index_name: Option<String>,
        /// 对每个项执行的动作。
        actions: Vec<ActionDecl>,
    },

    /// 立即退出当前循环。
    BreakLoop,

    /// 跳到循环的下一次迭代。
    ContinueLoop,

    /// 按 ID 跳转到指定动作。
    GotoAction {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// 要跳转到的动作 ID。
        #[serde(rename = "actionId")]
        action_id: String,
    },

    // ── 输出 ──

    /// 向用户发送消息。
    SendActivity {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        activity: SendActivityPayload,
    },

    // ── Agent 调用 ──

    /// 调用已注册的 Agent。
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

    // ── 工具调用 ──

    /// 直接调用函数工具，无需经过 AI Agent。
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

    /// 在 MCP（模型上下文协议）服务器上调用工具。
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

    /// 发送 HTTP 请求。
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

    // ── 人机交互 ──

    /// 向用户提问并存储回答。
    Question {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        question: QuestionPayload,
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    /// 向外部系统或进程请求输入。
    RequestExternalInput {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        prompt: QuestionPayload,
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    // ── 工作流控制 ──

    /// 终止工作流执行。
    EndWorkflow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },

    /// 结束当前对话。
    EndConversation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },

    /// 创建新的对话上下文。
    CreateConversation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(rename = "conversationId")]
        conversation_id: String,
    },

    // ── 对话（仅 C#） ──

    /// 向对话线程添加消息。
    AddConversationMessage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        message: MessagePayload,
    },
}

fn default_http_method() -> String {
    "GET".into()
}

// ── 辅助类型 ──

/// `ConditionGroup` 动作中的分支。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionBranch {
    /// 求值为 true/false 的 PowerFx 表达式。
    pub condition: String,
    /// 可选的分支标识符。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 此条件匹配时执行的动作。
    pub actions: Vec<ActionDecl>,
}

/// 对已注册 Agent 的引用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRef {
    /// Agent 名称（已注册的标识符）。
    pub name: String,
}

/// `SendActivity` 动作的负载。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendActivityPayload {
    /// 消息文本（字面量或 `=expression`）。
    pub text: serde_json::Value,
}

/// 人机交互动作的提问或提示。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionPayload {
    pub text: String,
}

/// Agent 调用的输入配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInput {
    /// 发送给 Agent 的消息。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    /// Agent 的附加参数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, serde_json::Value>>,
    /// 外部循环配置（持续执行直到条件满足）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_loop: Option<ExternalLoop>,
}

/// Agent 调用的外部循环配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalLoop {
    /// 继续循环的 PowerFx 条件。
    pub when: String,
}

/// Agent 调用的输出配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOutput {
    /// 存储 Agent 响应对象的路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_object: Option<String>,
    /// 存储对话消息的路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    /// 自动向用户发送响应。
    #[serde(default)]
    pub auto_send: Option<bool>,
}

/// 工具调用的输出配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// 存储工具结果的路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// 存储结果消息的路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<String>,
    /// 自动向用户发送结果。
    #[serde(default)]
    pub auto_send: Option<bool>,
}

/// HTTP 请求体变体。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HttpBody {
    /// JSON 请求体。
    Json { value: serde_json::Value },
    /// 原始字符串请求体。
    Raw { value: String },
    /// 无请求体。
    None,
}

/// 对话消息负载。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    /// 消息角色（例如 "user"、"assistant"、"system"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// 消息内容。
    pub content: String,
}

impl ActionDecl {
    /// 获取动作类型字符串（MAF 兼容）。
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

    /// 获取动作 ID（如存在）。
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

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schema::PropertySchema;

/// AI Agent 的工具声明，与 MAF AgentSchema v1.0 工具类型对齐。
///
/// MAF 定义了以下工具类型：
/// - `function` — OpenAI Function Calling
/// - `custom` — 工厂注册的自定义工具
/// - `web_search` — 网络搜索引擎工具
/// - `file_search` — 文件/向量搜索工具
/// - `mcp` — 模型上下文协议工具
/// - `openapi` — 基于 OpenAPI 规范的工具
/// - `code_interpreter` — 沙箱代码执行工具
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDecl {
    /// OpenAI Function Calling 工具。
    #[serde(rename = "function")]
    Function {
        /// 函数/工具名称。
        name: String,
        /// 人类可读的描述。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        /// 工具参数的 JSON Schema。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameters: Option<PropertySchema>,
        /// 从 inputSchema 属性到工具参数的绑定。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bindings: Vec<ToolBinding>,
    },

    /// 工厂注册的自定义工具。
    #[serde(rename = "custom")]
    Custom {
        /// 工具名称（用于工厂查找）。
        name: String,
        /// 人类可读的描述。
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        /// 转发给工厂的任意配置。
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        config: HashMap<String, serde_json::Value>,
    },

    /// 网络搜索工具（使用 Bing/Google/DuckDuckGo）。
    #[serde(rename = "web_search")]
    WebSearch,

    /// 文件/向量搜索工具。
    #[serde(rename = "file_search")]
    FileSearch {
        /// 用于定向搜索的向量存储 ID。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        vector_store_ids: Vec<String>,
    },

    /// 模型上下文协议（MCP）工具。
    #[serde(rename = "mcp")]
    Mcp {
        /// 工具展示名称。
        name: String,
        /// MCP 服务器 URL。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_url: Option<String>,
        /// MCP 服务器上的具体工具名称。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        /// 审批模式："always"、"never" 或 "specify"。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_mode: Option<String>,
    },

    /// 基于 OpenAPI 规范的工具。
    #[serde(rename = "openapi")]
    OpenApi {
        /// 工具展示名称。
        name: String,
        /// OpenAPI 规范的 URL。
        #[serde(rename = "specUrl")]
        spec_url: String,
        /// 可选的操作 ID，用于定位特定端点。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_id: Option<String>,
    },

    /// 沙箱代码解释器工具。
    #[serde(rename = "code_interpreter")]
    CodeInterpreter,
}

/// 从 inputSchema 属性到工具参数的绑定。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolBinding {
    /// 绑定名称。
    pub name: String,
    /// 输入属性的路径（例如 inputSchema 中的 "question"）。
    pub input: String,
}

impl ToolDecl {
    /// 获取工具名称（如适用）。
    pub fn name(&self) -> Option<&str> {
        match self {
            ToolDecl::Function { name, .. } => Some(name),
            ToolDecl::Custom { name, .. } => Some(name),
            ToolDecl::Mcp { name, .. } => Some(name),
            ToolDecl::OpenApi { name, .. } => Some(name),
            _ => None,
        }
    }

    /// 获取工具类型字符串。
    pub fn kind_str(&self) -> &'static str {
        match self {
            ToolDecl::Function { .. } => "function",
            ToolDecl::Custom { .. } => "custom",
            ToolDecl::WebSearch => "web_search",
            ToolDecl::FileSearch { .. } => "file_search",
            ToolDecl::Mcp { .. } => "mcp",
            ToolDecl::OpenApi { .. } => "openapi",
            ToolDecl::CodeInterpreter => "code_interpreter",
        }
    }

    /// 创建带名称和描述的函数工具。
    pub fn function(name: impl Into<String>, description: impl Into<String>) -> Self {
        ToolDecl::Function {
            name: name.into(),
            description: description.into(),
            parameters: None,
            bindings: Vec::new(),
        }
    }

    /// 创建带名称的自定义工具。
    pub fn custom(name: impl Into<String>) -> Self {
        ToolDecl::Custom {
            name: name.into(),
            description: String::new(),
            config: HashMap::new(),
        }
    }

    /// 创建 MCP 工具。
    pub fn mcp(name: impl Into<String>, server_url: impl Into<String>, tool_name: impl Into<String>) -> Self {
        ToolDecl::Mcp {
            name: name.into(),
            server_url: Some(server_url.into()),
            tool_name: Some(tool_name.into()),
            approval_mode: None,
        }
    }
}

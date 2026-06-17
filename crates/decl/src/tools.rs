use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schema::PropertySchema;

/// Tool declaration for an AI agent.
/// Aligns with MAF AgentSchema v1.0 tool types.
///
/// MAF defines these tool kinds:
/// - `function` — OpenAI Function Calling
/// - `custom` — Factory-registered custom tools
/// - `web_search` — Web search engine tool
/// - `file_search` — File/vector search tool
/// - `mcp` — Model Context Protocol tool
/// - `openapi` — OpenAPI specification-based tool
/// - `code_interpreter` — Sandbox code execution tool
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDecl {
    /// OpenAI Function Calling tool.
    #[serde(rename = "function")]
    Function {
        /// Function/tool name.
        name: String,
        /// Human-readable description.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        /// JSON Schema for the tool's parameters.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameters: Option<PropertySchema>,
        /// Bindings from inputSchema properties to tool arguments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bindings: Vec<ToolBinding>,
    },

    /// Factory-registered custom tool.
    #[serde(rename = "custom")]
    Custom {
        /// Tool name (used for factory lookup).
        name: String,
        /// Human-readable description.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        /// Arbitrary configuration forwarded to the factory.
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        config: HashMap<String, serde_json::Value>,
    },

    /// Web search tool (uses Bing/Google/DuckDuckGo).
    #[serde(rename = "web_search")]
    WebSearch,

    /// File/vector search tool.
    #[serde(rename = "file_search")]
    FileSearch {
        /// Vector store IDs for targeted search.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        vector_store_ids: Vec<String>,
    },

    /// Model Context Protocol (MCP) tool.
    #[serde(rename = "mcp")]
    Mcp {
        /// Tool display name.
        name: String,
        /// MCP server URL.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_url: Option<String>,
        /// Specific tool name on the MCP server.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        /// Approval mode: "always", "never", or "specify".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approval_mode: Option<String>,
    },

    /// OpenAPI-specification-based tool.
    #[serde(rename = "openapi")]
    OpenApi {
        /// Tool display name.
        name: String,
        /// URL to the OpenAPI specification.
        #[serde(rename = "specUrl")]
        spec_url: String,
        /// Optional operation ID to target a specific endpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_id: Option<String>,
    },

    /// Sandbox code interpreter tool.
    #[serde(rename = "code_interpreter")]
    CodeInterpreter,
}

/// Binding from an inputSchema property to a tool argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolBinding {
    /// Binding name.
    pub name: String,
    /// Path to the input property (e.g., "question" in inputSchema).
    pub input: String,
}

impl ToolDecl {
    /// Get the tool name, if applicable.
    pub fn name(&self) -> Option<&str> {
        match self {
            ToolDecl::Function { name, .. } => Some(name),
            ToolDecl::Custom { name, .. } => Some(name),
            ToolDecl::Mcp { name, .. } => Some(name),
            ToolDecl::OpenApi { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Get the tool kind string.
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

    /// Create a function tool with name and description.
    pub fn function(name: impl Into<String>, description: impl Into<String>) -> Self {
        ToolDecl::Function {
            name: name.into(),
            description: description.into(),
            parameters: None,
            bindings: Vec::new(),
        }
    }

    /// Create a custom tool with name.
    pub fn custom(name: impl Into<String>) -> Self {
        ToolDecl::Custom {
            name: name.into(),
            description: String::new(),
            config: HashMap::new(),
        }
    }

    /// Create an MCP tool.
    pub fn mcp(name: impl Into<String>, server_url: impl Into<String>, tool_name: impl Into<String>) -> Self {
        ToolDecl::Mcp {
            name: name.into(),
            server_url: Some(server_url.into()),
            tool_name: Some(tool_name.into()),
            approval_mode: None,
        }
    }
}

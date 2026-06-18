use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::ITool;
use rust_agent_framework::tools::{
    EditFile, FindFiles, InspectFile, ListFiles, MakeDirectory, MoveFile, ReadFile,
    RemovePath, RunCommand, SearchFile, WriteFile,
};
use rust_agent_mcp::McpServerClient;
use rust_agent_websearch::{WebFetch, WebSearch};

use crate::error::DeclError;
use crate::tools::ToolDecl;

/// 自定义工具工厂函数的类型别名。
pub type ToolFactoryFn =
    Box<dyn Fn(HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>> + Send + Sync>;

/// 将 `ToolDecl` 解析为具体的 `Arc<dyn ITool>`。
///
/// 支持所有 7 种 MAF 工具类型，内置解析器处理 `function`、`web_search` 和
/// `code_interpreter`。`custom`、`mcp`、`openapi` 和 `file_search`
/// 需要工厂注册或外部插件系统。
///
/// MCP 工具通过 `register_mcp_server()` 或 `register_mcp_server_arc()`
/// 注册已连接的 MCP 服务器实例来解析。
pub struct ToolResolver {
    /// 按名称键控的自定义工具工厂。
    factories: HashMap<String, ToolFactoryFn>,
    /// 已注册的 MCP 服务器客户端，按 server_url 键控。
    mcp_servers: HashMap<String, Arc<McpServerClient>>,
}

impl ToolResolver {
    /// 创建无自定义工厂的新工具解析器。
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
            mcp_servers: HashMap::new(),
        }
    }

    /// 注册自定义工具工厂。
    pub fn register_factory(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>>
            + Send
            + Sync
            + 'static,
    ) {
        self.factories.insert(name.into(), Box::new(factory));
    }

    /// 注册一个 MCP 服务器客户端，按 server_url 键控。
    ///
    /// 注册后，任何引用相同 `server_url` 的 `ToolDecl::Mcp` 声明
    /// 都将使用此客户端进行解析。
    pub fn register_mcp_server(
        &mut self,
        server_url: impl Into<String>,
        server: McpServerClient,
    ) {
        self.mcp_servers
            .insert(server_url.into(), Arc::new(server));
    }

    /// 使用已共享的 Arc 注册 MCP 服务器客户端。
    pub fn register_mcp_server_arc(
        &mut self,
        server_url: impl Into<String>,
        server: Arc<McpServerClient>,
    ) {
        self.mcp_servers.insert(server_url.into(), server);
    }

    /// 获取已注册的 MCP 服务器 URL 列表。
    pub fn mcp_server_urls(&self) -> Vec<&str> {
        self.mcp_servers.keys().map(|s| s.as_str()).collect()
    }

    /// 将单个工具声明解析为 ITool。
    pub async fn resolve(&self, tool: &ToolDecl) -> crate::Result<Arc<dyn ITool>> {
        match tool {
            ToolDecl::Function { name, description, parameters, bindings } => {
                resolve_function(name, description, parameters, bindings)
            }
            ToolDecl::Custom { name, config, .. } => {
                let factory = self.factories.get(name).ok_or_else(|| {
                    DeclError::Missing(format!(
                        "No factory registered for custom tool '{}'",
                        name
                    ))
                })?;
                factory(config.clone())
            }
            ToolDecl::WebSearch => Ok(Arc::new(WebSearch)),
            ToolDecl::FileSearch { .. } => Err(DeclError::Unsupported(
                "FileSearch tools require vector store integration".into(),
            )),
            ToolDecl::Mcp { name, server_url, tool_name, .. } => {
                resolve_mcp(&self.mcp_servers, name, server_url.as_deref(), tool_name.as_deref()).await
            }
            ToolDecl::OpenApi { .. } => Err(DeclError::Unsupported(
                "OpenAPI tools require spec parsing + HTTP client".into(),
            )),
            ToolDecl::CodeInterpreter => Err(DeclError::Unsupported(
                "CodeInterpreter requires sandbox execution environment".into(),
            )),
        }
    }

    /// 解析列表中的所有工具声明。
    pub async fn resolve_all(&self, tools: &[ToolDecl]) -> crate::Result<Vec<Arc<dyn ITool>>> {
        let mut resolved = Vec::with_capacity(tools.len());
        for tool in tools {
            resolved.push(self.resolve(tool).await?);
        }
        Ok(resolved)
    }
}

impl Default for ToolResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// 通过查找内置工具和框架工具来解析 Function 工具。
fn resolve_function(
    name: &str,
    _description: &str,
    _parameters: &Option<crate::schema::PropertySchema>,
    _bindings: &[crate::tools::ToolBinding],
) -> crate::Result<Arc<dyn ITool>> {
    // Map function names to built-in framework tools
    let tool: Arc<dyn ITool> = match name {
        "read_file" => Arc::new(ReadFile { scope: None }),
        "write_file" => Arc::new(WriteFile { scope: None }),
        "edit_file" => Arc::new(EditFile { scope: None }),
        "list_files" => Arc::new(ListFiles { scope: None }),
        "inspect_file" => Arc::new(InspectFile { scope: None }),
        "make_directory" => Arc::new(MakeDirectory { scope: None }),
        "remove_path" => Arc::new(RemovePath { scope: None }),
        "move_file" => Arc::new(MoveFile { scope: None }),
        "find_files" => Arc::new(FindFiles { scope: None }),
        "search_file" => Arc::new(SearchFile { scope: None }),
        "run_command" => Arc::new(RunCommand { scope: None, timeout_secs: None }),
        "web_search" => Arc::new(WebSearch),
        "web_fetch" => Arc::new(WebFetch),
        other => {
            return Err(DeclError::Unsupported(format!(
                "Unknown function tool '{}'",
                other
            )));
        }
    };
    Ok(tool)
}

/// 解析 MCP 工具声明为 `Arc<dyn ITool>`。
///
/// 从已注册的 MCP 服务器列表中查找指定的服务器 URL，
/// 发现其工具列表，并返回匹配的工具。如果指定了 `tool_name`，
/// 则精确匹配；否则使用声明中的 `name` 字段匹配。
async fn resolve_mcp(
    mcp_servers: &HashMap<String, Arc<McpServerClient>>,
    name: &str,
    server_url: Option<&str>,
    tool_name: Option<&str>,
) -> crate::Result<Arc<dyn ITool>> {
    let server_url = server_url.ok_or_else(|| {
        DeclError::Unsupported(
            "MCP tool declaration requires a server_url".into(),
        )
    })?;

    let server = mcp_servers.get(server_url).ok_or_else(|| {
        DeclError::Missing(format!(
            "No MCP server registered for URL '{}'. \
             Use ToolResolver::register_mcp_server() to register one.",
            server_url
        ))
    })?;

    let effective_tool_name = tool_name.unwrap_or(name);

    // Try to discover tools from the server and find the matching one
    let tools = server
        .discover_tools()
        .await
        .map_err(|e| DeclError::Resolution(format!(
            "Failed to discover tools from MCP server '{}': {}",
            server_url, e
        )))?;

    let mcp_tool = tools
        .into_iter()
        .find(|tool| tool.name() == effective_tool_name)
        .ok_or_else(|| {
            DeclError::Missing(format!(
                "MCP server '{}' does not expose a tool named '{}'",
                server_url, effective_tool_name
            ))
        })?;

    Ok(Arc::new(mcp_tool))
}

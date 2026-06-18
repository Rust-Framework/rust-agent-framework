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
/// 按 `(kind, name)` 二元组分派：
/// - `function`、`custom` → 查工厂映射
/// - `web`    → `web_search` / `web_fetch`
/// - `file`   → `read_file` / `write_file` / ... 11 个文件系统工具
/// - `code`   → `code_interpreter`
/// - `mcp`    → MCP 远程工具
/// - `openapi`→ OpenAPI 规范工具（需外部解析器）
pub struct ToolResolver {
    /// 按名称键控的自定义工具工厂（`function` 和 `custom` 共用）。
    factories: HashMap<String, ToolFactoryFn>,
    /// 已注册的 MCP 服务器客户端，按 server_url 键控。
    mcp_servers: HashMap<String, Arc<McpServerClient>>,
}

impl ToolResolver {
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

    /// 注册 MCP 服务器客户端。
    pub fn register_mcp_server(
        &mut self,
        server_url: impl Into<String>,
        server: McpServerClient,
    ) {
        self.mcp_servers
            .insert(server_url.into(), Arc::new(server));
    }

    /// 使用 Arc 注册 MCP 服务器客户端。
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

    /// 解析单个工具声明。
    pub async fn resolve(&self, tool: &ToolDecl) -> crate::Result<Arc<dyn ITool>> {
        match tool {
            // ── 用户注册 / 工厂注册 ──
            ToolDecl::Function { name, .. } => {
                self.lookup_factory(name, &HashMap::new())
            }
            ToolDecl::Custom { name, config, .. } => {
                self.lookup_factory(name, config)
            }

            // ── Web 工具 ──
            ToolDecl::Web { name, .. } => self.resolve_web(name),

            // ── 文件系统工具 ──
            ToolDecl::File { name, .. } => self.resolve_file(name),

            // ── 代码执行工具 ──
            ToolDecl::Code { name, .. } => self.resolve_code(name),

            // ── MCP ──
            ToolDecl::Mcp { name, server_url, tool_name, .. } => {
                resolve_mcp(&self.mcp_servers, name, server_url.as_deref(), tool_name.as_deref()).await
            }

            // ── OpenAPI ──
            ToolDecl::OpenApi { .. } => Err(DeclError::Unsupported(
                "OpenAPI tools require spec parsing + HTTP client".into(),
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

    // ── 内部方法 ──

    fn lookup_factory(&self, name: &str, config: &HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>> {
        let factory = self.factories.get(name).ok_or_else(|| {
            DeclError::Unsupported(format!(
                "Unknown tool '{}' — not a built-in and no factory registered. \
                 Built-in tools: kind=web (web_search, web_fetch), kind=file (read_file, etc.), kind=code (code_interpreter). \
                 User tools: register via with_tool() or register_factory().",
                name
            ))
        })?;
        factory(config.clone())
    }

    fn resolve_web(&self, name: &str) -> crate::Result<Arc<dyn ITool>> {
        match name {
            "web_search" => Ok(Arc::new(WebSearch)),
            "web_fetch" => Ok(Arc::new(WebFetch)),
            other => self.lookup_factory(other, &HashMap::new()),
        }
    }

    fn resolve_file(&self, name: &str) -> crate::Result<Arc<dyn ITool>> {
        match name {
            "read_file" => Ok(Arc::new(ReadFile { scope: None })),
            "write_file" => Ok(Arc::new(WriteFile { scope: None })),
            "edit_file" => Ok(Arc::new(EditFile { scope: None })),
            "list_files" => Ok(Arc::new(ListFiles { scope: None })),
            "inspect_file" => Ok(Arc::new(InspectFile { scope: None })),
            "make_directory" => Ok(Arc::new(MakeDirectory { scope: None })),
            "remove_path" => Ok(Arc::new(RemovePath { scope: None })),
            "move_file" => Ok(Arc::new(MoveFile { scope: None })),
            "find_files" => Ok(Arc::new(FindFiles { scope: None })),
            "search_file" => Ok(Arc::new(SearchFile { scope: None })),
            "run_command" => Ok(Arc::new(RunCommand { scope: None, timeout_secs: None })),
            other => self.lookup_factory(other, &HashMap::new()),
        }
    }

    fn resolve_code(&self, name: &str) -> crate::Result<Arc<dyn ITool>> {
        match name {
            "code_interpreter" => Err(DeclError::Unsupported(
                "CodeInterpreter requires sandbox execution environment".into(),
            )),
            other => self.lookup_factory(other, &HashMap::new()),
        }
    }
}

impl Default for ToolResolver {
    fn default() -> Self {
        Self::new()
    }
}

// ── MCP 解析 ──

async fn resolve_mcp(
    mcp_servers: &HashMap<String, Arc<McpServerClient>>,
    name: &str,
    server_url: Option<&str>,
    tool_name: Option<&str>,
) -> crate::Result<Arc<dyn ITool>> {
    let server_url = server_url.ok_or_else(|| {
        DeclError::Unsupported("MCP tool declaration requires a server_url".into())
    })?;

    let server = mcp_servers.get(server_url).ok_or_else(|| {
        DeclError::Missing(format!(
            "No MCP server registered for URL '{}'. \
             Use ToolResolver::register_mcp_server() to register one.",
            server_url
        ))
    })?;

    let effective_tool_name = tool_name.unwrap_or(name);

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

use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::ITool;
use rust_agent_framework::tools::{
    EditFile, FindFiles, InspectFile, ListFiles, MakeDirectory, MoveFile, ReadFile,
    RemovePath, RunCommand, SearchFile, WriteFile,
};
use rust_agent_mcp::McpServerClient;
#[cfg(feature = "web")]
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
    /// 工作流/Agent 级沙箱默认配置。
    sandbox_defaults: HashMap<String, serde_json::Value>,
}

impl ToolResolver {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
            mcp_servers: HashMap::new(),
            sandbox_defaults: HashMap::new(),
        }
    }

    pub fn set_sandbox_defaults(&mut self, defaults: HashMap<String, serde_json::Value>) {
        self.sandbox_defaults = defaults;
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
            ToolDecl::Web { name, .. } => match name.as_deref() {
                Some(n) => self.resolve_web(n),
                None => Err(DeclError::Unsupported(
                    "kind: web without name — use resolve_category() for bulk registration"
                        .into(),
                )),
            },

            // ── 文件系统工具 ──
            ToolDecl::File { name, .. } => match name.as_deref() {
                Some(n) => self.resolve_file(n),
                None => Err(DeclError::Unsupported(
                    "kind: file without name — use resolve_category() for bulk registration"
                        .into(),
                )),
            },

            // ── Shell 命令执行 ──
            ToolDecl::Shell { name, .. } => match name.as_deref() {
                Some(n) => self.resolve_shell(n),
                None => Err(DeclError::Unsupported(
                    "kind: shell without name — use resolve_category() for bulk registration"
                        .into(),
                )),
            },

            // ── 代码执行工具 ──
            ToolDecl::Code { name, config, .. } => match name.as_deref() {
                Some(n) => self.resolve_code(n, config),
                None => Err(DeclError::Unsupported(
                    "kind: code without name — use resolve_category() for bulk registration"
                        .into(),
                )),
            },

            // ── MCP ──
            ToolDecl::Mcp { name, server_url, tool_name, .. } => {
                resolve_mcp(&self.mcp_servers, name, server_url.as_deref(), tool_name.as_deref()).await
            }

            // ── OpenAPI ──
            ToolDecl::OpenApi {
                name,
                spec_url,
                operation_id,
                ..
            } => {
                #[cfg(feature = "openapi")]
                {
                    use rust_agent_openapi::{OpenApiToolConfig, OpenApiToolResolver};
                    return OpenApiToolResolver::resolve(&OpenApiToolConfig {
                        tool_name: name.clone(),
                        spec_url: spec_url.clone(),
                        operation_id: operation_id.clone(),
                        base_url: None,
                    })
                    .await
                    .map_err(DeclError::from);
                }
                #[cfg(not(feature = "openapi"))]
                {
                    let _ = (name, spec_url, operation_id);
                    Err(DeclError::Unsupported(
                        "OpenAPI tools require decl `openapi` feature and rust-agent-openapi crate"
                            .into(),
                    ))
                }
            }
        }
    }

    /// 解析列表中的所有工具声明。带 name-expansion 支持。
    pub async fn resolve_all(&self, tools: &[ToolDecl]) -> crate::Result<Vec<Arc<dyn ITool>>> {
        let mut resolved = Vec::new();
        for tool in tools {
            match tool.needs_expansion() {
                true => resolved.extend(self.resolve_category(tool).await?),
                false => resolved.push(self.resolve(tool).await?),
            }
        }
        Ok(resolved)
    }

    /// 判断是否需要展开为该分类下全部工具（委托给 ToolDecl::needs_expansion）。
    pub fn needs_expansion(&self, tool: &ToolDecl) -> bool {
        tool.needs_expansion()
    }

    /// 按分类展开全部工具——kind: web 无 name 时展开为 web_search + web_fetch 等。
    pub async fn resolve_category(&self, tool: &ToolDecl) -> crate::Result<Vec<Arc<dyn ITool>>> {
        match tool {
            ToolDecl::Web { .. } => Ok(vec![
                self.resolve_web("web_search")?,
                self.resolve_web("web_fetch")?,
            ]),
            ToolDecl::File { .. } => Ok(vec![
                self.resolve_file("read_file")?,
                self.resolve_file("write_file")?,
                self.resolve_file("edit_file")?,
                self.resolve_file("list_files")?,
                self.resolve_file("inspect_file")?,
                self.resolve_file("make_directory")?,
                self.resolve_file("remove_path")?,
                self.resolve_file("move_file")?,
                self.resolve_file("find_files")?,
                self.resolve_file("search_file")?,
            ]),
            ToolDecl::Shell { .. } => Ok(vec![
                self.resolve_shell("run_command")?,
            ]),
            ToolDecl::Code { config, .. } => Ok(vec![self.resolve_code("code_interpreter", config)?]),
            _ => Err(DeclError::Unsupported(
                "resolve_category only supported for web/file/code tools".into(),
            )),
        }
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
        #[cfg(feature = "web")]
        {
            match name {
                "web_search" => return Ok(Arc::new(WebSearch)),
                "web_fetch" => return Ok(Arc::new(WebFetch)),
                _ => {}
            }
        }
        #[cfg(not(feature = "web"))]
        if matches!(name, "web_search" | "web_fetch") {
            return Err(DeclError::Unsupported(
                "web tools require decl `web` feature and rust-agent-websearch crate".into(),
            ));
        }
        self.lookup_factory(name, &HashMap::new())
    }

    fn resolve_file(&self, name: &str) -> crate::Result<Arc<dyn ITool>> {
        match name {
            "read_file" => Ok(Arc::new(ReadFile::default())),
            "write_file" => Ok(Arc::new(WriteFile::default())),
            "edit_file" => Ok(Arc::new(EditFile::default())),
            "list_files" => Ok(Arc::new(ListFiles::default())),
            "inspect_file" => Ok(Arc::new(InspectFile::default())),
            "make_directory" => Ok(Arc::new(MakeDirectory::default())),
            "remove_path" => Ok(Arc::new(RemovePath::default())),
            "move_file" => Ok(Arc::new(MoveFile::default())),
            "find_files" => Ok(Arc::new(FindFiles::default())),
            "search_file" => Ok(Arc::new(SearchFile::default())),
            other => self.lookup_factory(other, &HashMap::new()),
        }
    }

    fn resolve_shell(&self, name: &str) -> crate::Result<Arc<dyn ITool>> {
        match name {
            "run_command" => Ok(Arc::new(RunCommand::default())),
            other => self.lookup_factory(other, &HashMap::new()),
        }
    }

    fn resolve_code(
        &self,
        name: &str,
        config: &HashMap<String, serde_json::Value>,
    ) -> crate::Result<Arc<dyn ITool>> {
        if let Ok(tool) = self.lookup_factory(name, config) {
            return Ok(tool);
        }
        if name == "code_interpreter" {
            let merged = crate::resolver::code_sandbox_executor::merge_sandbox_config(
                &self.sandbox_defaults,
                config,
            );
            return crate::sandbox_factory::build_code_interpreter(&merged);
        }
        self.lookup_factory(name, config)
    }

    /// 按 InvokeFunctionTool 的 functionName 解析工具。
    pub async fn resolve_by_function_name(&self, name: &str) -> crate::Result<Arc<dyn ITool>> {
        if let Ok(tool) = self.resolve_web(name) {
            return Ok(tool);
        }
        if let Ok(tool) = self.resolve_file(name) {
            return Ok(tool);
        }
        if let Ok(tool) = self.resolve_shell(name) {
            return Ok(tool);
        }
        if let Ok(tool) = self.resolve_code(name, &HashMap::new()) {
            return Ok(tool);
        }
        self.lookup_factory(name, &HashMap::new())
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

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rust_agent_core::ITool;
use rust_agent_framework::tools::{
    EditFile, FindFiles, InspectFile, ListFiles, MakeDirectory, MoveFile, ReadFile,
    RemovePath, RunCommand, SearchFile, WriteFile,
};
use rust_agent_websearch::{WebFetch, WebSearch};

use crate::error::DeclError;
use crate::tools::ToolDecl;

/// Type alias for a custom tool factory function.
pub type ToolFactoryFn =
    Box<dyn Fn(HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>> + Send + Sync>;

/// Resolve a `ToolDecl` into a concrete `Arc<dyn ITool>`.
///
/// Supports all 7 MAF tool kinds with the built-in resolver handling
/// `function`, `web_search`, and `code_interpreter`. `custom`, `mcp`,
/// `openapi`, and `file_search` require factory registration or external
/// plugin systems.
pub struct ToolResolver {
    /// Custom tool factories keyed by name.
    factories: HashMap<String, ToolFactoryFn>,
}

impl ToolResolver {
    /// Create a new tool resolver with no custom factories.
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a custom tool factory.
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

    /// Resolve a single tool declaration into an ITool.
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
            ToolDecl::Mcp { .. } => Err(DeclError::Unsupported(
                "MCP tools require MCP client integration".into(),
            )),
            ToolDecl::OpenApi { .. } => Err(DeclError::Unsupported(
                "OpenAPI tools require spec parsing + HTTP client".into(),
            )),
            ToolDecl::CodeInterpreter => Err(DeclError::Unsupported(
                "CodeInterpreter requires sandbox execution environment".into(),
            )),
        }
    }

    /// Resolve all tool declarations in a list.
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

/// Resolve a Function tool by looking up built-in tools and framework tools.
fn resolve_function(
    name: &str,
    _description: &str,
    _parameters: &Option<crate::schema::PropertySchema>,
    _bindings: &[crate::tools::ToolBinding],
) -> crate::Result<Arc<dyn ITool>> {
    // Map function names to built-in framework tools
    let tool: Arc<dyn ITool> = match name {
        "read_file" => Arc::new(ReadFile::new(Path::new("."))),
        "write_file" => Arc::new(WriteFile::new(Path::new("."))),
        "edit_file" => Arc::new(EditFile::new(Path::new("."))),
        "list_files" => Arc::new(ListFiles::new(Path::new("."))),
        "inspect_file" => Arc::new(InspectFile::new(Path::new("."))),
        "make_directory" => Arc::new(MakeDirectory::new(Path::new("."))),
        "remove_path" => Arc::new(RemovePath::new(Path::new("."))),
        "move_file" => Arc::new(MoveFile::new(Path::new("."))),
        "find_files" => Arc::new(FindFiles::new(Path::new("."))),
        "search_file" => Arc::new(SearchFile::new(Path::new("."))),
        "run_command" => Arc::new(RunCommand::new(Path::new("."))),
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

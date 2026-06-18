use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe_new, ScopeStatus};

const MAX_CONTENT_SIZE: usize = 1_000_000;

#[tool(description = "Creates a new file or overwrites an existing file with the given content.")]
pub struct WriteFile {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for WriteFile {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(WriteFile {
            scope: Some(scope),
        })
    }
}

impl WriteFile {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
            content: String,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!("Argument deserialization failed: {}", e))
        })?;

        if args.content.len() > MAX_CONTENT_SIZE {
            return Ok(ToolResult::error(format!(
                "Content size {} exceeds maximum of {} bytes",
                args.content.len(),
                MAX_CONTENT_SIZE,
            )));
        }

        let base_dir = self
            .scope
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let scope_root = self.scope.as_ref().map(|s| s.root.as_path());

        let (resolved, scope_status) =
            match resolve_safe_new(&base_dir, &args.path, scope_root) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolResult::error(format!("Path resolution failed: {}", e)));
                }
            };

        if let Some(ref scope) = self.scope {
            if scope.policy == ScopePolicy::DenyOutside
                && matches!(scope_status, ScopeStatus::OutsideScope)
            {
                return Ok(ToolResult::error(
                    "Access denied: path is outside workspace boundary",
                ));
            }
        }

        if let Some(parent) = resolved.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Ok(ToolResult::error(format!(
                        "Failed to create parent directory: {}",
                        e
                    )));
                }
            }
        }

        match std::fs::write(&resolved, &args.content) {
            Ok(_) => Ok(ToolResult::success(serde_json::json!({
                "path": args.path,
                "bytes_written": args.content.len(),
                "scope": scope_status.to_label(),
            }))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file: {}", e))),
        }
    }
}

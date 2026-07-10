use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe_new, ScopeStatus};

#[derive(Default)]
pub struct MakeDirectory {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for MakeDirectory {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(MakeDirectory {
            scope: Some(scope),
        })
    }
}

#[tool(
    description = "创建目录及其所有父目录（类似 mkdir -p）。",
    kind = "file",
    scope_tool = true
)]
impl MakeDirectory {
    async fn call(
        &self,
        #[param(desc = "目录路径（相对于工作区根目录，如 tests/fixtures）")] path: String,
    ) -> rust_agent_core::Result<ToolResult> {
        let base_dir = self
            .scope
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let scope_root = self.scope.as_ref().map(|s| s.root.as_path());

        let (resolved, scope_status) =
            match resolve_safe_new(&base_dir, &path, scope_root) {
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

        match std::fs::create_dir_all(&resolved) {
            Ok(_) => Ok(ToolResult::success(serde_json::json!({
                "path": path,
                "created": true,
                "scope": scope_status.to_label(),
            }))),
            Err(e) => Ok(ToolResult::error(format!("Failed to create directory: {}", e))),
        }
    }
}

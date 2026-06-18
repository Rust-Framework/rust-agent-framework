use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

pub struct RemovePath {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for RemovePath {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(RemovePath {
            scope: Some(scope),
        })
    }
}

#[tool(
    description = "删除指定路径的文件或目录。",
    kind = "file"
)]
impl RemovePath {
    async fn call(
        &self,
        #[param(desc = "要删除的文件或目录的绝对路径")] path: String,
    ) -> rust_agent_core::Result<ToolResult> {
        let base_dir = self
            .scope
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let scope_root = self.scope.as_ref().map(|s| s.root.as_path());

        let (resolved, scope_status) = match resolve_safe(&base_dir, &path, scope_root) {
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

        // Guard: 禁止删除 base_dir 本身或危险路径
        let canonical_base = base_dir
            .canonicalize()
            .unwrap_or_else(|_| base_dir.clone());
        let dangerous_dirs = vec![
            canonical_base.clone(),
            PathBuf::from("/"),
            PathBuf::from("C:\\"),
            dirs_next::home_dir().unwrap_or_default(),
        ];
        for dangerous in &dangerous_dirs {
            let max_components = if dangerous == &canonical_base {
                dangerous.components().count()
            } else {
                dangerous.components().count() + 1
            };
            if resolved == *dangerous
                || (resolved.starts_with(dangerous)
                    && resolved.components().count() <= max_components)
            {
                return Ok(ToolResult::error("Refusing to delete critical path"));
            }
        }

        let meta = match std::fs::symlink_metadata(&resolved) {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::error(format!("Failed to access path: {}", e))),
        };

        let result = if meta.is_dir() {
            std::fs::remove_dir_all(&resolved)
        } else {
            std::fs::remove_file(&resolved)
        };

        match result {
            Ok(_) => Ok(ToolResult::success(serde_json::json!({
                "path": path,
                "deleted": true,
                "scope": scope_status.to_label(),
            }))),
            Err(e) => Ok(ToolResult::error(format!("Failed to delete: {}", e))),
        }
    }
}

use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, resolve_safe_new, ScopeStatus};

pub struct MoveFile {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for MoveFile {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(MoveFile {
            scope: Some(scope),
        })
    }
}

#[tool(
    description = "移动或重命名文件或目录。",
    kind = "file"
)]
impl MoveFile {
    async fn call(
        &self,
        #[param(desc = "源文件的绝对路径")] from: String,
        #[param(desc = "目标文件的绝对路径")] to: String,
    ) -> rust_agent_core::Result<ToolResult> {
        let base_dir = self
            .scope
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let scope_root = self.scope.as_ref().map(|s| s.root.as_path());

        let (resolved_from, from_scope) =
            match resolve_safe(&base_dir, &from, scope_root) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "Path resolution failed for source: {}",
                        e
                    )));
                }
            };
        let (resolved_to, to_scope) =
            match resolve_safe_new(&base_dir, &to, scope_root) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "Path resolution failed for destination: {}",
                        e
                    )));
                }
            };

        // Use the wider scope for reporting
        let scope_label = if matches!(from_scope, ScopeStatus::OutsideScope)
            || matches!(to_scope, ScopeStatus::OutsideScope)
        {
            "outside_workspace"
        } else {
            from_scope.to_label()
        };

        if let Some(ref scope) = self.scope {
            if scope.policy == ScopePolicy::DenyOutside
                && (matches!(from_scope, ScopeStatus::OutsideScope)
                    || matches!(to_scope, ScopeStatus::OutsideScope))
            {
                return Ok(ToolResult::error(
                    "Access denied: path is outside workspace boundary",
                ));
            }
        }

        if let Some(parent) = resolved_to.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return Ok(ToolResult::error(format!(
                        "Failed to create destination parent directory: {}",
                        e
                    )));
                }
            }
        }

        if resolved_to.try_exists().unwrap_or(false) {
            return Ok(ToolResult::error(format!(
                "Destination already exists: {}",
                resolved_to.display()
            )));
        }

        match std::fs::rename(&resolved_from, &resolved_to) {
            Ok(_) => Ok(ToolResult::success(serde_json::json!({
                "from": from,
                "to": to,
                "moved": true,
                "scope": scope_label,
            }))),
            Err(e) => Ok(ToolResult::error(format!("Failed to move/rename: {}", e))),
        }
    }
}

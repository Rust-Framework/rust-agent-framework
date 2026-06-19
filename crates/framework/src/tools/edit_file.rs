use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

#[derive(Default)]
pub struct EditFile {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for EditFile {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(EditFile {
            scope: Some(scope),
        })
    }
}

#[tool(
    description = "精确替换已有文件中的字符串。提供 old_str（要查找的原文本）和 new_str（替换为新文本）。old_str 必须在文件中唯一匹配一个连续块。",
    kind = "file"
)]
impl EditFile {
    async fn call(
        &self,
        #[param(desc = "要编辑的文件的绝对路径")] path: String,
        #[param(desc = "文件中待替换的原文本（须唯一且连续，含空白和换行）")] old_str: String,
        #[param(desc = "替换后的新文本")] new_str: String,
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

        let original = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read file: {}", e))),
        };

        if old_str.is_empty() {
            return Ok(ToolResult::error("old_str must not be empty"));
        }

        let occurrences: Vec<usize> =
            original.match_indices(&old_str).map(|(i, _)| i).collect();

        if occurrences.is_empty() {
            return Ok(ToolResult::error(
                "old_str not found in the file. Make sure you copied the exact text including whitespace and newlines.",
            ));
        }

        if occurrences.len() > 1 {
            let positions: Vec<String> = occurrences
                .iter()
                .take(5)
                .map(|i| format!("byte offset {}", i))
                .collect();
            return Ok(ToolResult::error(format!(
                "old_str is not unique — found {} occurrences (e.g. at {}). Provide more surrounding context.",
                occurrences.len(),
                positions.join(", ")
            )));
        }

        let edited = original.replacen(&old_str, &new_str, 1);

        match std::fs::write(&resolved, &edited) {
            Ok(_) => Ok(ToolResult::success(serde_json::json!({
                "path": path,
                "replaced": true,
                "scope": scope_status.to_label(),
            }))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file: {}", e))),
        }
    }
}

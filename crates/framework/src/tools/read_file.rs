use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

const MAX_FILE_SIZE: u64 = 512 * 1024;
const MAX_LINE_LEN: usize = 2000;

pub struct ReadFile {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for ReadFile {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(ReadFile {
            scope: Some(scope),
        })
    }
}

#[tool(
    description = "读取本地文件系统中的文件内容，支持通过 offset/limit 指定行范围。",
    kind = "file"
)]
impl ReadFile {
    async fn call(
        &self,
        #[param(desc = "文件的绝对路径")] path: String,
        #[param(desc = "起始行号（从 1 开始计数，可选）")] offset: Option<i64>,
        #[param(desc = "最多读取行数（可选）")] limit: Option<i64>,
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

        let meta = match std::fs::metadata(&resolved) {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::error(format!("Failed to access path: {}", e))),
        };

        if !meta.is_file() {
            return Ok(ToolResult::error("Path is not a file"));
        }

        if meta.len() > MAX_FILE_SIZE {
            return Ok(ToolResult::error(format!(
                "File too large ({} bytes, max {})",
                meta.len(),
                MAX_FILE_SIZE
            )));
        }

        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read file: {}", e))),
        };

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len() as i64;

        let start = offset.unwrap_or(1).max(1) as usize;
        let start_idx = (start - 1).min(all_lines.len());

        let end_idx = match limit {
            Some(l) if l > 0 => (start_idx + l as usize).min(all_lines.len()),
            _ => all_lines.len(),
        };

        let selected: Vec<String> = all_lines[start_idx..end_idx]
            .iter()
            .map(|line| {
                if line.len() > MAX_LINE_LEN {
                    format!("{}...[truncated]", &line[..MAX_LINE_LEN])
                } else {
                    line.to_string()
                }
            })
            .collect();

        let mut output = selected.join("\n");
        let truncated = end_idx < all_lines.len();
        if truncated {
            output.push_str("\n\n[truncated — use offset/limit to read more]");
        }

        Ok(ToolResult::success(serde_json::json!({
            "path": path,
            "content": output,
            "total_lines": total_lines,
            "start_line": start_idx as i64 + 1,
            "end_line": end_idx as i64,
            "truncated": truncated,
            "scope": scope_status.to_label(),
        })))
    }
}

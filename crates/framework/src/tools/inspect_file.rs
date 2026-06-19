use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::resolve_safe;

#[derive(Default)]
pub struct InspectFile {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for InspectFile {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(InspectFile {
            scope: Some(scope),
        })
    }
}

fn format_system_time(t: Option<std::time::SystemTime>) -> String {
    t.and_then(|t| {
        let dt: chrono::DateTime<chrono::Utc> = t.into();
        Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    })
    .unwrap_or_else(|| "unknown".to_string())
}

#[tool(
    description = "返回文件或目录的元数据：类型、字节大小、修改时间、权限。",
    kind = "file",
    scope_tool = true
)]
impl InspectFile {
    async fn call(
        &self,
        #[param(desc = "文件或目录的绝对路径")] path: String,
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

        let meta = match std::fs::metadata(&resolved) {
            Ok(m) => m,
            Err(e) => return Ok(ToolResult::error(format!("Failed to access path: {}", e))),
        };

        let file_type = if meta.is_dir() {
            "dir"
        } else if meta.is_file() {
            "file"
        } else {
            "unknown"
        };

        Ok(ToolResult::success(serde_json::json!({
            "path": path,
            "type": file_type,
            "size": meta.len(),
            "readonly": meta.permissions().readonly(),
            "modified": format_system_time(meta.modified().ok()),
            "created": format_system_time(meta.created().ok()),
            "scope": scope_status.to_label(),
        })))
    }
}

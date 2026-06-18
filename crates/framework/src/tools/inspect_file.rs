use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

#[tool(description = "Returns metadata about a file or directory: type, size in bytes, modification time, permissions.")]
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

impl InspectFile {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!("Argument deserialization failed: {}", e))
        })?;

        let base_dir = self
            .scope
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let scope_root = self.scope.as_ref().map(|s| s.root.as_path());

        let (resolved, scope_status) = match resolve_safe(&base_dir, &args.path, scope_root) {
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
            "path": args.path,
            "type": file_type,
            "size": meta.len(),
            "readonly": meta.permissions().readonly(),
            "modified": format_system_time(meta.modified().ok()),
            "created": format_system_time(meta.created().ok()),
            "scope": scope_status.to_label(),
        })))
    }
}

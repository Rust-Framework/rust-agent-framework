use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::resolve_safe;

pub struct ListFiles {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for ListFiles {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(ListFiles {
            scope: Some(scope),
        })
    }
}

#[tool(
    description = "列出指定路径下的文件和目录。返回每个条目的名称、类型（file/dir/symlink）和大小。",
    kind = "file"
)]
impl ListFiles {
    async fn call(
        &self,
        #[param(desc = "目录的绝对路径")] path: String,
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

        let entries = match std::fs::read_dir(&resolved) {
            Ok(rd) => rd,
            Err(e) => {
                return Ok(ToolResult::error(format!("Failed to read directory: {}", e)));
            }
        };

        let mut items: Vec<serde_json::Value> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let mut item = serde_json::json!({"name": name});

            match entry.file_type() {
                Ok(ft) => {
                    if ft.is_dir() {
                        item["type"] = serde_json::Value::String("dir".into());
                    } else if ft.is_symlink() {
                        item["type"] = serde_json::Value::String("symlink".into());
                    } else {
                        item["type"] = serde_json::Value::String("file".into());
                    }
                }
                Err(_) => {
                    item["type"] = serde_json::Value::String("unknown".into());
                }
            }

            if let Ok(meta) = entry.metadata() {
                item["size"] = serde_json::json!(meta.len());
            }

            items.push(item);
        }

        items.sort_by(|a, b| {
            let a_type = a["type"].as_str().unwrap_or("");
            let b_type = b["type"].as_str().unwrap_or("");
            let a_is_dir = a_type == "dir";
            let b_is_dir = b_type == "dir";
            if a_is_dir != b_is_dir {
                b_is_dir.cmp(&a_is_dir)
            } else {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
            }
        });

        Ok(ToolResult::success(serde_json::json!({
            "path": path,
            "entries": items,
            "count": items.len(),
            "scope": scope_status.to_label(),
        })))
    }
}

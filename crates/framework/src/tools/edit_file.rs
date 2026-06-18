use std::path::{Path, PathBuf};
use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

#[tool(description = "Performs exact string replacement in an existing file. Provide old_str (the exact text to find) and new_str (the replacement). The old_str must uniquely match a contiguous block of lines in the file.")]
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

impl EditFile {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
            old_str: String,
            new_str: String,
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

        if args.old_str.is_empty() {
            return Ok(ToolResult::error("old_str must not be empty"));
        }

        let occurrences: Vec<usize> =
            original.match_indices(&args.old_str).map(|(i, _)| i).collect();

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

        let edited = original.replacen(&args.old_str, &args.new_str, 1);

        match std::fs::write(&resolved, &edited) {
            Ok(_) => Ok(ToolResult::success(serde_json::json!({
                "path": args.path,
                "replaced": true,
                "scope": scope_status.to_label(),
            }))),
            Err(e) => Ok(ToolResult::error(format!("Failed to write file: {}", e))),
        }
    }
}

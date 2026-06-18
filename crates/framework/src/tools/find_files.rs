use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

const MAX_RESULTS: usize = 500;

#[tool(description = "Finds files matching a glob pattern under a directory.")]
pub struct FindFiles {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for FindFiles {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(FindFiles {
            scope: Some(scope),
        })
    }
}

impl FindFiles {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            pattern: String,
            directory: Option<String>,
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

        let (base, scope_status) = match args.directory {
            Some(d) => match resolve_safe(&base_dir, &d, scope_root) {
                Ok((r, s)) => (r.to_string_lossy().replace('\\', "/"), s),
                Err(e) => {
                    return Ok(ToolResult::error(format!("Path resolution failed: {}", e)));
                }
            },
            None => (
                base_dir.to_string_lossy().replace('\\', "/"),
                ScopeStatus::NotApplicable,
            ),
        };

        let full_pattern = format!("{}/{}", base.trim_end_matches('/'), args.pattern);

        let glob = match glob::glob(&full_pattern) {
            Ok(g) => g,
            Err(e) => return Ok(ToolResult::error(format!("Invalid glob pattern: {}", e))),
        };

        let mut results: Vec<String> = Vec::new();
        for entry in glob.flatten() {
            if results.len() >= MAX_RESULTS {
                break;
            }
            results.push(entry.to_string_lossy().to_string());
        }

        let truncated = results.len() >= MAX_RESULTS;
        results.sort();

        Ok(ToolResult::success(serde_json::json!({
            "pattern": args.pattern,
            "directory": base,
            "results": results,
            "count": results.len(),
            "truncated": truncated,
            "scope": scope_status.to_label(),
        })))
    }
}

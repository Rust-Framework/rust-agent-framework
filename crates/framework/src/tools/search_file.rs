use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::{IScopeTool, ITool, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

const MAX_MATCHES: usize = 200;
const MAX_LINE_DISPLAY: usize = 300;

#[tool(description = "Searches file contents using a regex pattern. Returns matching lines with file path and line number.")]
pub struct SearchFile {
    pub scope: Option<Arc<WorkspaceScope>>,
}

impl IScopeTool for SearchFile {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(SearchFile {
            scope: Some(scope),
        })
    }
}

impl SearchFile {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            pattern: String,
            directory: String,
            include: Option<String>,
            case_insensitive: Option<bool>,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!("Argument deserialization failed: {}", e))
        })?;

        let case_insensitive = args.case_insensitive.unwrap_or(false);
        let base_dir = self
            .scope
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let scope_root = self.scope.as_ref().map(|s| s.root.as_path());

        let (resolved_dir, scope_status) =
            match resolve_safe(&base_dir, &args.directory, scope_root) {
                Ok(r) => r,
                Err(e) => {
                    return Ok(ToolResult::error(format!("Path resolution failed: {}", e)));
                }
            };

        let regex = if case_insensitive {
            match regex::bytes::RegexBuilder::new(&format!("(?i){}", args.pattern))
                .build()
            {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(format!("Invalid regex: {}", e))),
            }
        } else {
            match regex::bytes::Regex::new(&args.pattern) {
                Ok(r) => r,
                Err(e) => return Ok(ToolResult::error(format!("Invalid regex: {}", e))),
            }
        };

        let walker = walkdir::WalkDir::new(&resolved_dir)
            .max_depth(20)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                e.depth() == 0
                    || !e.file_name()
                        .to_str()
                        .map(|s| s.starts_with('.'))
                        .unwrap_or(false)
            });

        let include_pattern = args.include.as_ref().map(|inc| {
            format!(
                "{}/{}",
                resolved_dir
                    .to_string_lossy()
                    .replace('\\', "/")
                    .trim_end_matches('/'),
                inc
            )
        });

        let mut matches: Vec<serde_json::Value> = Vec::new();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            if !entry.file_type().is_file() {
                continue;
            }

            if let Some(ref pattern) = include_pattern {
                let path = entry.path().to_string_lossy().replace('\\', "/");
                if !glob::Pattern::new(pattern).map_or(false, |p| p.matches(&path)) {
                    continue;
                }
            }

            let contents = match std::fs::read(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if contents.iter().take(8192).any(|&b| b == 0) {
                continue;
            }

            for (line_num, line) in contents.split(|&b| b == b'\n').enumerate() {
                if regex.is_match(line) {
                    if matches.len() >= MAX_MATCHES {
                        break;
                    }
                    let display = String::from_utf8_lossy(line);
                    let truncated: String = if display.len() > MAX_LINE_DISPLAY {
                        format!(
                            "{}...",
                            display.chars().take(MAX_LINE_DISPLAY).collect::<String>()
                        )
                    } else {
                        display.to_string()
                    };
                    matches.push(serde_json::json!({
                        "file": entry.path().to_string_lossy(),
                        "line": line_num + 1,
                        "content": truncated,
                    }));
                }
            }
            if matches.len() >= MAX_MATCHES {
                break;
            }
        }

        let truncated = matches.len() >= MAX_MATCHES;

        Ok(ToolResult::success(serde_json::json!({
            "pattern": args.pattern,
            "directory": args.directory,
            "matches": matches,
            "total": matches.len(),
            "truncated": truncated,
            "scope": scope_status.to_label(),
        })))
    }
}

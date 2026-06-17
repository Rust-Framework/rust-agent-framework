use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::resolve_safe;
use super::{err_response, ok_response};

const MAX_RESULTS: usize = 500;

/// Finds files matching a glob pattern from a directory.
///
/// The directory is resolved against `base_dir`:
/// - If `directory` is absolute → pass through unchanged.
/// - If `directory` is relative → join with `base_dir`.
/// - If `directory` is None → use `base_dir` directly.
pub struct FindFiles {
    base_dir: PathBuf,
}

impl FindFiles {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(&self, pattern: String, directory: Option<String>) -> String {
        let base = match directory {
            Some(d) => {
                match resolve_safe(&self.base_dir, &d) {
                    Ok(r) => r.to_string_lossy().replace('\\', "/"),
                    Err(e) => return err_response(&format!("Path resolution failed: {}", e)),
                }
            }
            None => self.base_dir.to_string_lossy().replace('\\', "/"),
        };
        let full_pattern = format!(
            "{}/{}",
            base.trim_end_matches('/'),
            pattern
        );

        let glob = match glob::glob(&full_pattern) {
            Ok(g) => g,
            Err(e) => return err_response(&format!("Invalid glob pattern: {}", e)),
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

        ok_response(serde_json::json!({
            "pattern": pattern,
            "directory": base,
            "matches": results,
            "count": results.len(),
            "truncated": truncated,
        }))
    }
}

impl Default for FindFiles {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for FindFiles {
    fn name(&self) -> &str {
        "find_files"
    }

    fn description(&self) -> &str {
        "Finds files matching a glob pattern (e.g. '**/*.rs'). Returns matching file paths."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern (e.g. '**/*.rs', 'src/*.ts')"
                },
                "directory": {
                    "type": "string",
                    "description": "Root directory to search from (optional, defaults to the agent's working directory)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            pattern: String,
            directory: Option<String>,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self.call(args.pattern, args.directory).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_find_rs_files() {
        let result = FindFiles::default()
            .execute(serde_json::json!({"pattern": "**/*.rs", "directory": "src"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["count"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_find_files_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();

        let tool = FindFiles::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"pattern": "*.rs"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["count"], 1);
    }
}

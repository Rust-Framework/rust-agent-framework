use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::resolve_safe;
use super::{err_response, ok_response};

const MAX_MATCHES: usize = 200;
const MAX_LINE_DISPLAY: usize = 300;

/// Searches file contents using a regex pattern.
///
/// The directory is resolved against `base_dir`:
/// - Absolute paths pass through unchanged.
/// - Relative paths are joined to `base_dir`.
pub struct SearchFile {
    base_dir: PathBuf,
}

impl SearchFile {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(
        &self,
        pattern: String,
        directory: String,
        include: Option<String>,
        case_insensitive: Option<bool>,
    ) -> String {
        let case_insensitive = case_insensitive.unwrap_or(false);
        let resolved_dir = match resolve_safe(&self.base_dir, &directory) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed: {}", e)),
        };

        let regex = if case_insensitive {
            match regex::bytes::RegexBuilder::new(&format!("(?i){}", pattern))
                .build()
            {
                Ok(r) => r,
                Err(e) => return err_response(&format!("Invalid regex pattern: {}", e)),
            }
        } else {
            match regex::bytes::Regex::new(&pattern) {
                Ok(r) => r,
                Err(e) => return err_response(&format!("Invalid regex pattern: {}", e)),
            }
        };

        let walker = walkdir::WalkDir::new(&resolved_dir)
            .max_depth(20)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                // Never skip the root directory; only filter subdirectories
                // that start with '.' (hidden dirs like .git)
                e.depth() == 0 || !e.file_name().to_str().map(|s| s.starts_with('.')).unwrap_or(false)
            });

        // Compile include glob if provided
        let include_pattern = include.as_ref().map(|inc| {
            format!(
                "{}/{}",
                resolved_dir.to_string_lossy().replace('\\', "/").trim_end_matches('/'),
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

            // Filter by include glob
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

            // Skip binary files (check for null bytes in first 8KB)
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
                        format!("{}...", display.chars().take(MAX_LINE_DISPLAY).collect::<String>())
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

        ok_response(serde_json::json!({
            "pattern": pattern,
            "directory": directory,
            "matches": matches,
            "total": matches.len(),
            "truncated": truncated,
        }))
    }
}

impl Default for SearchFile {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for SearchFile {
    fn name(&self) -> &str {
        "search_file"
    }

    fn description(&self) -> &str {
        "Searches file contents using a regex pattern. Returns matching lines with file path and line number."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "directory": {
                    "type": "string",
                    "description": "Directory to search recursively (absolute, or relative to the agent's working directory)"
                },
                "include": {
                    "type": "string",
                    "description": "Glob pattern to filter files to include (e.g. '*.rs')"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case insensitive search (default: false)"
                }
            },
            "required": ["pattern", "directory"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            pattern: String,
            directory: String,
            include: Option<String>,
            case_insensitive: Option<bool>,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self.call(args.pattern, args.directory, args.include, args.case_insensitive).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_rust_code() {
        let result = SearchFile::default()
            .execute(serde_json::json!({
                "pattern": "fn test_",
                "directory": "src/tools",
                "include": "*.rs",
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["total"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_search_file_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("code.rs"), "fn hello() {}\nfn test_stuff() {}\n").unwrap();

        let tool = SearchFile::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"pattern": "fn test_", "directory": "."}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["total"], 1);
    }
}

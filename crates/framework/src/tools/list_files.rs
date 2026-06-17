use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::resolve_safe;
use super::{err_response, ok_response};

/// Lists files and directories at the given path.
///
/// Paths are resolved against `base_dir`:
/// - Absolute paths pass through unchanged.
/// - Relative paths are joined to `base_dir`.
pub struct ListFiles {
    base_dir: PathBuf,
}

impl ListFiles {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(&self, path: String) -> String {
        let resolved = match resolve_safe(&self.base_dir, &path) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed: {}", e)),
        };

        let entries = match std::fs::read_dir(&resolved) {
            Ok(rd) => rd,
            Err(e) => return err_response(&format!("Failed to read directory: {}", e)),
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

        // Sort: dirs first, then files, alphabetical
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

        ok_response(serde_json::json!({
            "path": path,
            "entries": items,
            "count": items.len(),
        }))
    }
}

impl Default for ListFiles {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for ListFiles {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "Lists files and directories at the given path. Returns name, type (file/dir/symlink), and size for each entry."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory (absolute, or relative to the agent's working directory)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self.call(args.path).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_files_non_existent() {
        let result = ListFiles::default()
            .execute(serde_json::json!({"path": "/nonexistent_dir"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn test_list_files_cwd() {
        let result = ListFiles::default()
            .execute(serde_json::json!({"path": "."}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["count"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_list_files_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let tool = ListFiles::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"path": "."}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["count"], 2);
    }
}

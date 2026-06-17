use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::resolve_safe;
use super::{err_response, ok_response};

/// Returns metadata about a file or directory: type, size in bytes, modification time, permissions.
///
/// Paths are resolved against `base_dir`:
/// - Absolute paths pass through unchanged.
/// - Relative paths are joined to `base_dir`.
pub struct InspectFile {
    base_dir: PathBuf,
}

impl InspectFile {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(&self, path: String) -> String {
        let resolved = match resolve_safe(&self.base_dir, &path) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed: {}", e)),
        };

        let meta = match std::fs::metadata(&resolved) {
            Ok(m) => m,
            Err(e) => return err_response(&format!("Failed to access path: {}", e)),
        };

        let file_type = if meta.is_dir() {
            "dir"
        } else if meta.is_file() {
            "file"
        } else {
            "unknown"
        };

        let size = meta.len();
        let readonly = meta.permissions().readonly();
        let modified = format_system_time(meta.modified().ok());
        let created = format_system_time(meta.created().ok());

        ok_response(serde_json::json!({
            "path": path,
            "type": file_type,
            "size": size,
            "readonly": readonly,
            "modified": modified,
            "created": created,
        }))
    }
}

fn format_system_time(t: Option<std::time::SystemTime>) -> String {
    t.and_then(|t| {
        let dt: chrono::DateTime<chrono::Utc> = t.into();
        Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    })
    .unwrap_or_else(|| "unknown".to_string())
}

impl Default for InspectFile {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for InspectFile {
    fn name(&self) -> &str {
        "inspect_file"
    }

    fn description(&self) -> &str {
        "Returns metadata about a file or directory: type, size in bytes, modification time, permissions."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file or directory (absolute, or relative to the agent's working directory)"
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
    async fn test_inspect_file_non_existent() {
        let result = InspectFile::default()
            .execute(serde_json::json!({"path": "/this_path_does_not_exist_12345abcde"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn test_inspect_file_cargo_toml() {
        let result = InspectFile::default()
            .execute(serde_json::json!({"path": "Cargo.toml"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["type"], "file");
    }

    #[tokio::test]
    async fn test_inspect_file_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "hello").unwrap();

        let tool = InspectFile::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"path": "f.txt"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["type"], "file");
        assert_eq!(v["data"]["size"], 5);
    }
}

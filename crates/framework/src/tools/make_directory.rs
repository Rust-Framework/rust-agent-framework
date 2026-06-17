use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::{err_response, ok_response};

/// Creates a directory and all parent directories if they don't exist (like mkdir -p).
///
/// Paths are resolved against `base_dir`:
/// - Absolute paths pass through unchanged.
/// - Relative paths are joined to `base_dir`.
pub struct MakeDirectory {
    base_dir: PathBuf,
}

impl MakeDirectory {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() { p.to_path_buf() }
        else if path.is_empty() || path == "." { self.base_dir.clone() }
        else { self.base_dir.join(p) }
    }

    async fn call(&self, path: String) -> String {
        let resolved = self.resolve(&path);

        match std::fs::create_dir_all(&resolved) {
            Ok(_) => ok_response(serde_json::json!({
                "path": path,
                "created": true,
            })),
            Err(e) => err_response(&format!("Failed to create directory: {}", e)),
        }
    }
}

impl Default for MakeDirectory {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for MakeDirectory {
    fn name(&self) -> &str {
        "make_directory"
    }

    fn description(&self) -> &str {
        "Creates a directory and all parent directories if they don't exist (like mkdir -p)."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path of the directory to create (absolute, or relative to the agent's working directory)"
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
    async fn test_make_and_cleanup_dir() {
        let tmp = "target/test_make_dir/subdir";
        let _ = std::fs::remove_dir_all("target/test_make_dir");

        let result = MakeDirectory::default()
            .execute(serde_json::json!({"path": tmp}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        assert!(std::fs::metadata(tmp).unwrap().is_dir());

        let _ = std::fs::remove_dir_all("target/test_make_dir");
    }

    #[tokio::test]
    async fn test_make_directory_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();

        let tool = MakeDirectory::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"path": "sub/deep"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(dir.path().join("sub/deep").is_dir());
    }
}

use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::resolve_safe;
use super::{err_response, ok_response};

/// Deletes a file or directory at the specified path.
///
/// Paths are resolved against `base_dir`:
/// - Absolute paths pass through unchanged.
/// - Relative paths are joined to `base_dir`.
pub struct RemovePath {
    base_dir: PathBuf,
}

impl RemovePath {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(&self, path: String) -> String {
        let resolved = match resolve_safe(&self.base_dir, &path) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed: {}", e)),
        };

        // Guard: refuse to delete base_dir itself or dangerous paths
        let canonical_base = self
            .base_dir
            .canonicalize()
            .unwrap_or_else(|_| self.base_dir.clone());
        let dangerous_dirs = vec![
            canonical_base.clone(),
            PathBuf::from("/"),
            PathBuf::from("C:\\"),
            dirs_next::home_dir().unwrap_or_default(),
        ];
        for dangerous in &dangerous_dirs {
            // For the base_dir itself, only block exact matches (don't block
            // legitimate files/dirs that live inside the working directory).
            // For system paths (/, C:\, home), also block direct children as
            // an extra safety measure.
            let max_components = if dangerous == &canonical_base {
                dangerous.components().count()
            } else {
                dangerous.components().count() + 1
            };
            if resolved == *dangerous
                || (resolved.starts_with(dangerous)
                    && resolved.components().count() <= max_components)
            {
                return err_response("Refusing to delete critical path");
            }
        }

        let meta = match std::fs::symlink_metadata(&resolved) {
            Ok(m) => m,
            Err(e) => return err_response(&format!("Failed to access path: {}", e)),
        };

        let result = if meta.is_dir() {
            std::fs::remove_dir_all(&resolved)
        } else {
            std::fs::remove_file(&resolved)
        };

        match result {
            Ok(_) => ok_response(serde_json::json!({
                "path": path,
                "deleted": true,
            })),
            Err(e) => err_response(&format!("Failed to delete: {}", e)),
        }
    }
}

impl Default for RemovePath {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for RemovePath {
    fn name(&self) -> &str {
        "remove_path"
    }

    fn description(&self) -> &str {
        "Deletes a file or directory at the specified path."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file or directory to delete (absolute, or relative to the agent's working directory)"
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
    async fn test_remove_file() {
        let tmp = "target/test_remove.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(tmp, "data").unwrap();
        assert!(std::fs::metadata(tmp).is_ok());

        let result = RemovePath::default()
            .execute(serde_json::json!({"path": tmp}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(std::fs::metadata(tmp).is_err());
    }

    #[tokio::test]
    async fn test_remove_dir() {
        let tmp = "target/test_remove_dir";
        std::fs::create_dir_all(tmp).unwrap();

        let result = RemovePath::default()
            .execute(serde_json::json!({"path": tmp}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(std::fs::metadata(tmp).is_err());
    }

    #[tokio::test]
    async fn test_remove_path_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tmp.txt"), "data").unwrap();
        assert!(dir.path().join("tmp.txt").exists());

        let tool = RemovePath::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"path": "tmp.txt"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(!dir.path().join("tmp.txt").exists());
    }
}

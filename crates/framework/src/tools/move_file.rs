use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::{resolve_safe, resolve_safe_new};
use super::{err_response, ok_response};

/// 移动或重命名文件或目录。
///
/// `from` 和 `to` 路径均相对于 `base_dir` 解析：
/// - 绝对路径直接使用。
/// - 相对路径拼接至 `base_dir`。
pub struct MoveFile {
    base_dir: PathBuf,
}

impl MoveFile {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(&self, from: String, to: String) -> String {
        let resolved_from = match resolve_safe(&self.base_dir, &from) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed for source: {}", e)),
        };
        let resolved_to = match resolve_safe_new(&self.base_dir, &to) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed for destination: {}", e)),
        };

        // Ensure parent directory of destination exists
        if let Some(parent) = resolved_to.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return err_response(&format!("Failed to create destination parent directory: {}", e));
                }
            }
        }

        // Refuse to overwrite existing destination
        if resolved_to.try_exists().unwrap_or(false) {
            return err_response(&format!(
                "Destination already exists: {}",
                resolved_to.display()
            ));
        }

        match std::fs::rename(&resolved_from, &resolved_to) {
            Ok(_) => ok_response(serde_json::json!({
                "from": from,
                "to": to,
                "moved": true,
            })),
            Err(e) => err_response(&format!("Failed to move/rename: {}", e)),
        }
    }
}

impl Default for MoveFile {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for MoveFile {
    fn name(&self) -> &str {
        "move_file"
    }

    fn description(&self) -> &str {
        "Moves or renames a file or directory."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "from": {
                    "type": "string",
                    "description": "Source path (absolute, or relative to the agent's working directory)"
                },
                "to": {
                    "type": "string",
                    "description": "Destination path (absolute, or relative to the agent's working directory)"
                }
            },
            "required": ["from", "to"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            from: String,
            to: String,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self.call(args.from, args.to).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_move_file() {
        let src = "target/test_move_src.txt";
        let dst = "target/test_move_dst.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(src, "data").unwrap();
        let _ = std::fs::remove_file(dst);

        let result = MoveFile::default()
            .execute(serde_json::json!({"from": src, "to": dst}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        assert!(std::fs::metadata(src).is_err());
        assert_eq!(std::fs::read_to_string(dst).unwrap(), "data");

        let _ = std::fs::remove_file(dst);
    }

    #[tokio::test]
    async fn test_move_file_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap();

        let tool = MoveFile::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"from": "a.txt", "to": "b.txt"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(!dir.path().join("a.txt").exists());
        assert_eq!(std::fs::read_to_string(dir.path().join("b.txt")).unwrap(), "hello");
    }
}

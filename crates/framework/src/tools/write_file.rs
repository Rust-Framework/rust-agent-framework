use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::resolve_safe_new;
use super::{err_response, ok_response};

/// Maximum file content size (1 MB) to prevent excessive memory usage.
const MAX_CONTENT_SIZE: usize = 1_000_000;

/// 创建新文件或覆盖已有文件的内容。
///
/// 路径相对于 `base_dir` 解析：
/// - 绝对路径直接使用。
/// - 相对路径拼接至 `base_dir`。
pub struct WriteFile {
    base_dir: PathBuf,
}

impl WriteFile {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(&self, path: String, content: String) -> String {
        if content.len() > MAX_CONTENT_SIZE {
            return err_response(&format!(
                "Content size {} exceeds maximum of {} bytes ({} MB)",
                content.len(),
                MAX_CONTENT_SIZE,
                MAX_CONTENT_SIZE / 1_000_000,
            ));
        }

        let resolved = match resolve_safe_new(&self.base_dir, &path) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed: {}", e)),
        };

        // Ensure parent directory exists
        if let Some(parent) = resolved.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return err_response(&format!("Failed to create parent directory: {}", e));
                }
            }
        }

        match std::fs::write(&resolved, &content) {
            Ok(_) => ok_response(serde_json::json!({
                "path": path,
                "bytes_written": content.len(),
            })),
            Err(e) => err_response(&format!("Failed to write file: {}", e)),
        }
    }
}

/// 默认：base_dir 为进程当前工作目录（向后兼容）。
impl Default for WriteFile {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Creates a new file or overwrites an existing file with the given content."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (absolute, or relative to the agent's working directory)"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
            content: String,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self.call(args.path, args.content).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_and_cleanup() {
        let tmp = "target/test_write_file.txt";
        let _ = std::fs::remove_file(tmp);

        let result = WriteFile::default()
            .execute(serde_json::json!({"path": tmp, "content": "hello world"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        let content = std::fs::read_to_string(tmp).unwrap();
        assert_eq!(content, "hello world");

        let _ = std::fs::remove_file(tmp);
    }

    #[tokio::test]
    async fn test_write_file_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();

        let tool = WriteFile::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"path": "new_file.txt", "content": "test content"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        let content = std::fs::read_to_string(dir.path().join("new_file.txt")).unwrap();
        assert_eq!(content, "test content");
    }

    #[tokio::test]
    async fn test_write_file_absolute_bypasses_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        let abs_path = dir.path().join("abs_file.txt");
        let abs_str = abs_path.to_string_lossy().to_string();

        // Use a different base_dir — absolute path should still resolve
        let tool = WriteFile::new(std::env::temp_dir());
        let result = tool
            .execute(serde_json::json!({"path": abs_str, "content": "absolute"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(std::fs::read_to_string(&abs_path).unwrap(), "absolute");

        let _ = std::fs::remove_file(&abs_path);
    }
}

use std::path::PathBuf;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::path_guard::resolve_safe;
use super::{err_response, ok_response};

const MAX_FILE_SIZE: u64 = 512 * 1024; // 512 KB
const MAX_LINE_LEN: usize = 2000; // truncate very long lines

/// 从本地文件系统读取文件内容。支持通过 offset/limit 指定行范围。
///
/// 路径相对于 `base_dir` 解析：
/// - 绝对路径直接使用。
/// - 相对路径拼接至 `base_dir`。
pub struct ReadFile {
    base_dir: PathBuf,
}

impl ReadFile {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    async fn call(&self, path: String, offset: Option<i64>, limit: Option<i64>) -> String {
        let resolved = match resolve_safe(&self.base_dir, &path) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Path resolution failed: {}", e)),
        };

        let meta = match std::fs::metadata(&resolved) {
            Ok(m) => m,
            Err(e) => return err_response(&format!("Failed to access path: {}", e)),
        };

        if !meta.is_file() {
            return err_response("Path is not a file");
        }

        if meta.len() > MAX_FILE_SIZE {
            return err_response(&format!(
                "File too large ({} bytes, max {})",
                meta.len(),
                MAX_FILE_SIZE
            ));
        }

        let content = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => return err_response(&format!("Failed to read file: {}", e)),
        };

        let all_lines: Vec<&str> = content.lines().collect();
        let total_lines = all_lines.len() as i64;

        let start = offset.unwrap_or(1).max(1) as usize;
        let start_idx = (start - 1).min(all_lines.len());

        let end_idx = match limit {
            Some(l) if l > 0 => (start_idx + l as usize).min(all_lines.len()),
            _ => all_lines.len(),
        };

        let selected: Vec<String> = all_lines[start_idx..end_idx]
            .iter()
            .map(|line| {
                if line.len() > MAX_LINE_LEN {
                    format!("{}...[truncated]", &line[..MAX_LINE_LEN])
                } else {
                    line.to_string()
                }
            })
            .collect();

        let mut output = selected.join("\n");
        let truncated = end_idx < all_lines.len();
        if truncated {
            output.push_str("\n\n[truncated — use offset/limit to read more]");
        }

        return ok_response(serde_json::json!({
            "path": path,
            "content": output,
            "total_lines": total_lines,
            "start_line": start_idx as i64 + 1,
            "end_line": end_idx as i64,
            "truncated": truncated,
        }));
    }
}

/// 默认：base_dir 为进程当前工作目录（向后兼容）。
impl Default for ReadFile {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Reads a file from the local filesystem. Supports line range via offset/limit."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (absolute, or relative to the agent's working directory)"
                },
                "offset": {
                    "type": "integer",
                    "description": "Starting line number (1-based, optional)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (optional)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
            offset: Option<i64>,
            limit: Option<i64>,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self.call(args.path, args.offset, args.limit).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_read_file_non_existent() {
        let result = ReadFile::default()
            .execute(serde_json::json!({"path": "/nonexistent/file.txt"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn test_read_file_cargo_toml() {
        let result = ReadFile::default()
            .execute(serde_json::json!({"path": "Cargo.toml"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["content"].as_str().unwrap().contains("rust-agent-framework"));
    }

    #[tokio::test]
    async fn test_read_file_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "hello\nworld").unwrap();

        let tool = ReadFile::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"path": "data.txt"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["content"], "hello\nworld");
    }

    #[tokio::test]
    async fn test_read_file_absolute_bypasses_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), "relative").unwrap();
        let abs_path = dir.path().join("data.txt");
        let abs_str = abs_path.to_string_lossy().to_string();

        // Use a different base_dir — absolute path should still resolve
        let tool = ReadFile::new(std::env::temp_dir());
        let result = tool
            .execute(serde_json::json!({"path": abs_str}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
    }
}

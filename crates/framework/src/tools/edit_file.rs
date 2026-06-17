use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::{err_response, ok_response};

/// Performs exact string replacement in an existing file.
///
/// Paths are resolved against `base_dir`:
/// - Absolute paths pass through unchanged.
/// - Relative paths are joined to `base_dir`.
pub struct EditFile {
    base_dir: PathBuf,
}

impl EditFile {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self { base_dir: base_dir.into() }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() { p.to_path_buf() }
        else if path.is_empty() || path == "." { self.base_dir.clone() }
        else { self.base_dir.join(p) }
    }

    async fn call(&self, path: String, old_str: String, new_str: String) -> String {
        let resolved = self.resolve(&path);

        let original = match std::fs::read_to_string(&resolved) {
            Ok(c) => c,
            Err(e) => return err_response(&format!("Failed to read file: {}", e)),
        };

        if old_str.is_empty() {
            return err_response("old_str must not be empty");
        }

        // Count occurrences
        let occurrences: Vec<usize> = original.match_indices(&old_str).map(|(i, _)| i).collect();

        if occurrences.is_empty() {
            return err_response(&format!(
                "old_str not found in the file. Make sure you copied the exact text including whitespace and newlines."
            ));
        }

        if occurrences.len() > 1 {
            let positions: Vec<String> = occurrences
                .iter()
                .take(5)
                .map(|i| format!("byte offset {}", i))
                .collect();
            return err_response(&format!(
                "old_str is not unique — found {} occurrences (e.g. at {}). Provide more surrounding context to make the match unique.",
                occurrences.len(),
                positions.join(", ")
            ));
        }

        let edited = original.replacen(&old_str, &new_str, 1);

        match std::fs::write(&resolved, &edited) {
            Ok(_) => ok_response(serde_json::json!({
                "path": path,
                "replaced": true,
            })),
            Err(e) => err_response(&format!("Failed to write file: {}", e)),
        }
    }
}

impl Default for EditFile {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Performs exact string replacement in an existing file. Provide old_str (the exact text to find) and new_str (the replacement). The old_str must uniquely match a contiguous block of lines in the file."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit (absolute, or relative to the agent's working directory)"
                },
                "old_str": {
                    "type": "string",
                    "description": "Exact text to find in the file (must be unique and contiguous)"
                },
                "new_str": {
                    "type": "string",
                    "description": "Text to replace it with"
                }
            },
            "required": ["path", "old_str", "new_str"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            path: String,
            old_str: String,
            new_str: String,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self.call(args.path, args.old_str, args.new_str).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_edit_file_basic() {
        let tmp = "target/test_edit_file.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(tmp, "line a\nline b\nline c\n").unwrap();

        let result = EditFile::default()
            .execute(serde_json::json!({
                "path": tmp,
                "old_str": "line b",
                "new_str": "line X"
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        let content = std::fs::read_to_string(tmp).unwrap();
        assert_eq!(content, "line a\nline X\nline c\n");

        let _ = std::fs::remove_file(tmp);
    }

    #[tokio::test]
    async fn test_edit_file_not_unique() {
        let tmp = "target/test_edit_file_dup.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(tmp, "dup\ndup\n").unwrap();

        let result = EditFile::default()
            .execute(serde_json::json!({
                "path": tmp,
                "old_str": "dup",
                "new_str": "x"
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("not unique"));

        let _ = std::fs::remove_file(tmp);
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let tmp = "target/test_edit_file_nf.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(tmp, "hello\n").unwrap();

        let result = EditFile::default()
            .execute(serde_json::json!({
                "path": tmp,
                "old_str": "not there",
                "new_str": "x"
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);

        let _ = std::fs::remove_file(tmp);
    }

    #[tokio::test]
    async fn test_edit_file_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "A B C").unwrap();

        let tool = EditFile::new(dir.path());
        let result = tool
            .execute(serde_json::json!({"path": "f.txt", "old_str": "B", "new_str": "X"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(std::fs::read_to_string(dir.path().join("f.txt")).unwrap(), "A X C");
    }
}

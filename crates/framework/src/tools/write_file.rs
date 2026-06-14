use rust_agent_macros::tool;

use super::{err_response, ok_response};

#[tool(description = "Creates a new file or overwrites an existing file with the given content.")]
async fn write_file(
    #[param(desc = "Absolute path to the file")] path: String,
    #[param(desc = "Content to write to the file")] content: String,
) -> String {
    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return err_response(&format!("Failed to create parent directory: {}", e));
            }
        }
    }

    match std::fs::write(&path, &content) {
        Ok(_) => ok_response(serde_json::json!({
            "path": path,
            "bytes_written": content.len(),
        })),
        Err(e) => err_response(&format!("Failed to write file: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_write_and_cleanup() {
        let tmp = "target/test_write_file.txt";
        let _ = std::fs::remove_file(tmp);

        let result = WriteFile
            .execute(serde_json::json!({"path": tmp, "content": "hello world"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        let content = std::fs::read_to_string(tmp).unwrap();
        assert_eq!(content, "hello world");

        let _ = std::fs::remove_file(tmp);
    }
}

use rust_agent_macros::tool;

use super::{err_response, ok_response};

#[tool(description = "Moves or renames a file or directory.")]
async fn move_file(
    #[param(desc = "Source absolute path")] from: String,
    #[param(desc = "Destination absolute path")] to: String,
) -> String {
    // Ensure parent directory of destination exists
    if let Some(parent) = std::path::Path::new(&to).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return err_response(&format!("Failed to create destination parent directory: {}", e));
            }
        }
    }

    match std::fs::rename(&from, &to) {
        Ok(_) => ok_response(serde_json::json!({
            "from": from,
            "to": to,
            "moved": true,
        })),
        Err(e) => err_response(&format!("Failed to move/rename: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_move_file() {
        let src = "target/test_move_src.txt";
        let dst = "target/test_move_dst.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(src, "data").unwrap();
        let _ = std::fs::remove_file(dst);

        let result = MoveFile
            .execute(serde_json::json!({"from": src, "to": dst}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        assert!(std::fs::metadata(src).is_err());
        assert_eq!(std::fs::read_to_string(dst).unwrap(), "data");

        let _ = std::fs::remove_file(dst);
    }
}

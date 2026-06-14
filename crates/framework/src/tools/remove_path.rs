use rust_agent_macros::tool;

use super::{err_response, ok_response};

#[tool(description = "Deletes a file or directory at the specified path.")]
async fn remove_path(
    #[param(desc = "Absolute path to the file or directory to delete")] path: String,
) -> String {
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) => return err_response(&format!("Failed to access path: {}", e)),
    };

    let result = if meta.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };

    match result {
        Ok(_) => ok_response(serde_json::json!({
            "path": path,
            "deleted": true,
        })),
        Err(e) => err_response(&format!("Failed to delete: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_remove_file() {
        let tmp = "target/test_remove.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(tmp, "data").unwrap();
        assert!(std::fs::metadata(tmp).is_ok());

        let result = RemovePath
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

        let result = RemovePath
            .execute(serde_json::json!({"path": tmp}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(std::fs::metadata(tmp).is_err());
    }
}

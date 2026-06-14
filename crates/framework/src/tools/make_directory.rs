use rust_agent_macros::tool;

use super::{err_response, ok_response};

#[tool(description = "Creates a directory and all parent directories if they don't exist (like mkdir -p).")]
async fn make_directory(
    #[param(desc = "Absolute path of the directory to create")] path: String,
) -> String {
    match std::fs::create_dir_all(&path) {
        Ok(_) => ok_response(serde_json::json!({
            "path": path,
            "created": true,
        })),
        Err(e) => err_response(&format!("Failed to create directory: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_make_and_cleanup_dir() {
        let tmp = "target/test_make_dir/subdir";
        let _ = std::fs::remove_dir_all("target/test_make_dir");

        let result = MakeDirectory
            .execute(serde_json::json!({"path": tmp}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);

        assert!(std::fs::metadata(tmp).unwrap().is_dir());

        let _ = std::fs::remove_dir_all("target/test_make_dir");
    }
}

use rust_agent_macros::tool;

use super::{err_response, ok_response};

#[tool(description = "Returns metadata about a file or directory: type, size in bytes, modification time, permissions.")]
async fn inspect_file(
    #[param(desc = "Absolute path to the file or directory")] path: String,
) -> String {
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return err_response(&format!("Failed to access path: {}", e)),
    };

    let file_type = if meta.is_dir() {
        "dir"
    } else if meta.is_file() {
        "file"
    } else {
        "unknown"
    };

    let size = meta.len();
    let readonly = meta.permissions().readonly();

    let modified = format_system_time(meta.modified().ok());
    let created = format_system_time(meta.created().ok());

    ok_response(serde_json::json!({
        "path": path,
        "type": file_type,
        "size": size,
        "readonly": readonly,
        "modified": modified,
        "created": created,
    }))
}

fn format_system_time(t: Option<std::time::SystemTime>) -> String {
    t.and_then(|t| {
        let dt: chrono::DateTime<chrono::Utc> = t.into();
        Some(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    })
    .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_inspect_file_non_existent() {
        let result = InspectFile
            .execute(serde_json::json!({"path": "/this_path_does_not_exist_12345abcde"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn test_inspect_file_cargo_toml() {
        let result = InspectFile
            .execute(serde_json::json!({"path": "Cargo.toml"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["type"], "file");
    }
}

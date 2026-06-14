use rust_agent_macros::tool;

use super::{err_response, ok_response};

#[tool(description = "Lists files and directories at the given path. Returns name, type (file/dir/symlink), and size for each entry.")]
async fn list_files(
    #[param(desc = "Absolute path to the directory")] path: String,
) -> String {
    let entries = match std::fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(e) => return err_response(&format!("Failed to read directory: {}", e)),
    };

    let mut items: Vec<serde_json::Value> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let mut item = serde_json::json!({"name": name});

        match entry.file_type() {
            Ok(ft) => {
                if ft.is_dir() {
                    item["type"] = serde_json::Value::String("dir".into());
                } else if ft.is_symlink() {
                    item["type"] = serde_json::Value::String("symlink".into());
                } else {
                    item["type"] = serde_json::Value::String("file".into());
                }
            }
            Err(_) => {
                item["type"] = serde_json::Value::String("unknown".into());
            }
        }

        if let Ok(meta) = entry.metadata() {
            item["size"] = serde_json::json!(meta.len());
        }

        items.push(item);
    }

    // Sort: dirs first, then files, alphabetical
    items.sort_by(|a, b| {
        let a_type = a["type"].as_str().unwrap_or("");
        let b_type = b["type"].as_str().unwrap_or("");
        let a_is_dir = a_type == "dir";
        let b_is_dir = b_type == "dir";
        if a_is_dir != b_is_dir {
            b_is_dir.cmp(&a_is_dir) // dirs before files
        } else {
            a["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
        }
    });

    ok_response(serde_json::json!({
        "path": path,
        "entries": items,
        "count": items.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_list_files_non_existent() {
        let result = ListFiles
            .execute(serde_json::json!({"path": "/nonexistent_dir"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn test_list_files_cwd() {
        let result = ListFiles
            .execute(serde_json::json!({"path": "."}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["count"].as_u64().unwrap() > 0);
    }
}

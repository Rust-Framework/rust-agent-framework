use rust_agent_macros::tool;

use super::{err_response, ok_response};

#[tool(description = "Performs exact string replacement in an existing file. Provide old_str (the exact text to find) and new_str (the replacement). The old_str must uniquely match a contiguous block of lines in the file.")]
async fn edit_file(
    #[param(desc = "Absolute path to the file to edit")] path: String,
    #[param(desc = "Exact text to find in the file (must be unique and contiguous)")] old_str: String,
    #[param(desc = "Text to replace it with")] new_str: String,
) -> String {
    let original = match std::fs::read_to_string(&path) {
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

    match std::fs::write(&path, &edited) {
        Ok(_) => ok_response(serde_json::json!({
            "path": path,
            "replaced": true,
        })),
        Err(e) => err_response(&format!("Failed to write file: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_edit_file_basic() {
        let tmp = "target/test_edit_file.txt";
        std::fs::create_dir_all("target").unwrap();
        std::fs::write(tmp, "line a\nline b\nline c\n").unwrap();

        // exact match
        let result = EditFile
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

        let result = EditFile
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

        let result = EditFile
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
}

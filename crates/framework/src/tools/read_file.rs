use rust_agent_macros::tool;

use super::{err_response, ok_response};

const MAX_FILE_SIZE: u64 = 512 * 1024; // 512 KB
const MAX_LINE_LEN: usize = 2000; // truncate very long lines

#[tool(description = "Reads a file from the local filesystem. Supports line range via offset/limit.")]
async fn read_file(
    #[param(desc = "Absolute path to the file")] path: String,
    #[param(desc = "Starting line number (1-based, optional)")] offset: Option<i64>,
    #[param(desc = "Maximum number of lines to read (optional)")] limit: Option<i64>,
) -> String {
    let meta = match std::fs::metadata(&path) {
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

    let content = match std::fs::read_to_string(&path) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_read_file_non_existent() {
        let result = ReadFile
            .execute(serde_json::json!({"path": "/nonexistent/file.txt"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn test_read_file_cargo_toml() {
        let result = ReadFile
            .execute(serde_json::json!({"path": "Cargo.toml"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["content"].as_str().unwrap().contains("rust-agent-framework"));
    }
}

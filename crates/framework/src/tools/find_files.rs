use rust_agent_macros::tool;

use super::{err_response, ok_response};

const MAX_RESULTS: usize = 500;

#[tool(description = "Finds files matching a glob pattern (e.g. '**/*.rs'). Returns matching file paths.")]
async fn find_files(
    #[param(desc = "Glob pattern (e.g. '**/*.rs', 'src/*.ts')")] pattern: String,
    #[param(desc = "Root directory to search from (optional, defaults to current working directory)")] directory: Option<String>,
) -> String {
    let base = directory.unwrap_or_else(|| ".".to_string());
    let full_pattern = format!(
        "{}/{}",
        base.replace('\\', "/").trim_end_matches('/'),
        pattern
    );

    let glob = match glob::glob(&full_pattern) {
        Ok(g) => g,
        Err(e) => return err_response(&format!("Invalid glob pattern: {}", e)),
    };

    let mut results: Vec<String> = Vec::new();
    for entry in glob.flatten() {
        if results.len() >= MAX_RESULTS {
            break;
        }
        results.push(entry.to_string_lossy().to_string());
    }

    let truncated = results.len() >= MAX_RESULTS;
    results.sort();

    ok_response(serde_json::json!({
        "pattern": pattern,
        "directory": base,
        "matches": results,
        "count": results.len(),
        "truncated": truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_find_rs_files() {
        let result = FindFiles
            .execute(serde_json::json!({"pattern": "**/*.rs", "directory": "src"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["count"].as_u64().unwrap() > 0);
    }
}

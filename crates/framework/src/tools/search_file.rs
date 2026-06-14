use rust_agent_macros::tool;

use super::{err_response, ok_response};

const MAX_MATCHES: usize = 200;
const MAX_LINE_DISPLAY: usize = 300;

#[tool(description = "Searches file contents using a regex pattern. Returns matching lines with file path and line number.")]
async fn search_file(
    #[param(desc = "Regular expression pattern to search for")] pattern: String,
    #[param(desc = "Directory to search recursively")] directory: String,
    #[param(desc = "Glob pattern to filter files to include (e.g. '*.rs')")] include: Option<String>,
    #[param(desc = "Case insensitive search (default: false)")] case_insensitive: Option<bool>,
) -> String {
    let case_insensitive = case_insensitive.unwrap_or(false);

    let regex = if case_insensitive {
        match regex::bytes::RegexBuilder::new(&format!("(?i){}", pattern))
            .build()
        {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Invalid regex pattern: {}", e)),
        }
    } else {
        match regex::bytes::Regex::new(&pattern) {
            Ok(r) => r,
            Err(e) => return err_response(&format!("Invalid regex pattern: {}", e)),
        }
    };

    let walker = walkdir::WalkDir::new(&directory)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            // Skip hidden directories
            !e.file_name().to_str().map(|s| s.starts_with('.')).unwrap_or(false)
        });

    // Compile include glob if provided
    let include_pattern = include.as_ref().map(|inc| {
        format!(
            "{}/{}",
            directory.replace('\\', "/").trim_end_matches('/'),
            inc
        )
    });

    let mut matches: Vec<serde_json::Value> = Vec::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().is_file() {
            continue;
        }

        // Filter by include glob
        if let Some(ref pattern) = include_pattern {
            let path = entry.path().to_string_lossy().replace('\\', "/");
            if !glob::Pattern::new(pattern).map_or(false, |p| p.matches(&path)) {
                continue;
            }
        }

        let contents = match std::fs::read(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Skip binary files (check for null bytes in first 8KB)
        if contents.iter().take(8192).any(|&b| b == 0) {
            continue;
        }

        for (line_num, line) in contents.split(|&b| b == b'\n').enumerate() {
            if regex.is_match(line) {
                if matches.len() >= MAX_MATCHES {
                    break;
                }
                let display = String::from_utf8_lossy(line);
                let truncated: String = if display.len() > MAX_LINE_DISPLAY {
                    format!("{}...", &display[..MAX_LINE_DISPLAY])
                } else {
                    display.to_string()
                };
                matches.push(serde_json::json!({
                    "file": entry.path().to_string_lossy(),
                    "line": line_num + 1,
                    "content": truncated,
                }));
            }
        }
        if matches.len() >= MAX_MATCHES {
            break;
        }
    }

    let truncated = matches.len() >= MAX_MATCHES;

    ok_response(serde_json::json!({
        "pattern": pattern,
        "directory": directory,
        "matches": matches,
        "total": matches.len(),
        "truncated": truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_search_rust_code() {
        let result = SearchFile
            .execute(serde_json::json!({
                "pattern": "fn test_",
                "directory": "src/tools",
                "include": "*.rs",
            }))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["total"].as_u64().unwrap() > 0);
    }
}

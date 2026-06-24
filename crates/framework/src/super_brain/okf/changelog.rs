use std::path::Path;

use chrono::Utc;

/// Append a consolidation event row to bundle-level `log.md` (OKF changelog).
pub fn append_consolidation_entry(
    memory_dir: &Path,
    status: &str,
    updated_files: &[String],
    session_id: Option<&str>,
) -> std::io::Result<()> {
    let log_path = memory_dir.join("log.md");
    if !log_path.exists() {
        return Ok(());
    }

    let date = Utc::now().format("%Y-%m-%d").to_string();
    let session = session_id.unwrap_or("-");
    let detail = if updated_files.is_empty() {
        status.to_string()
    } else {
        format!("{status}: {}", updated_files.join(", "))
    };
    let row = format!("| {date} | {session} | {detail} |\n");

    let mut content = std::fs::read_to_string(&log_path)?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&row);
    std::fs::write(log_path, content)
}

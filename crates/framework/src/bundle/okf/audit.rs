//! 合并后对 assets/ 树的索引完整性检查。

use std::path::{Path, PathBuf};

/// 在 `assets/` 下检测到缺失或损坏的索引条目。
#[derive(Debug, Clone)]
pub struct IndexGap {
    pub path: PathBuf,
    pub reason: String,
}

/// 扫描 `memory_dir/assets/` 中的索引链缺口。
pub fn scan_index_gaps(memory_dir: &Path) -> Vec<IndexGap> {
    let assets = memory_dir.join("assets");
    if !assets.is_dir() {
        return Vec::new();
    }

    let mut gaps = Vec::new();
    let mut topic_dirs: Vec<PathBuf> = Vec::new();

    for topic_entry in walk_dirs(&assets) {
        let topic_name = topic_entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if topic_name == "INDEX.md" || topic_entry.file_name() == Some(std::ffi::OsStr::new("INDEX.md")) {
            continue;
        }
        if !topic_entry.is_dir() {
            continue;
        }
        topic_dirs.push(topic_entry.clone());

        let topic_index = topic_entry.join("INDEX.md");
        if !topic_index.exists() {
            gaps.push(IndexGap {
                path: topic_index,
                reason: "topic INDEX.md missing".into(),
            });
        }

        for chapter_entry in walk_dirs(&topic_entry) {
            if !chapter_entry.is_dir() {
                continue;
            }
            let chapter_index = chapter_entry.join("INDEX.md");
            let has_leaf = has_markdown_leaf(&chapter_entry);
            if has_leaf && !chapter_index.exists() {
                gaps.push(IndexGap {
                    path: chapter_index,
                    reason: "chapter INDEX.md missing but leaf .md files exist".into(),
                });
            }
        }
    }

    if !topic_dirs.is_empty() {
        let root_index = assets.join("INDEX.md");
        let root_empty = root_index
            .metadata()
            .map(|m| m.len() == 0)
            .unwrap_or(true);
        if root_empty {
            gaps.push(IndexGap {
                path: root_index,
                reason: "assets/INDEX.md empty but topics exist".into(),
            });
        }
    }

    gaps
}

fn walk_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.push(path);
            }
        }
    }
    out
}

fn has_markdown_leaf(dir: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".md") && name != "INDEX.md" {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// 格式化索引缺口用于 CLI / 日志输出。
pub fn format_index_gaps(gaps: &[IndexGap]) -> String {
    if gaps.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = gaps
        .iter()
        .map(|g| format!("  - {} ({})", g.path.display(), g.reason))
        .collect();
    format!("INDEX_GAP:\n{}", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_empty_root_index_with_topics() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join("assets");
        fs::create_dir_all(assets.join("topic")).unwrap();
        fs::write(assets.join("topic").join("leaf.md"), "x").unwrap();

        let gaps = scan_index_gaps(dir.path());
        assert!(gaps.iter().any(|g| g.reason.contains("assets/INDEX.md")));
    }
}

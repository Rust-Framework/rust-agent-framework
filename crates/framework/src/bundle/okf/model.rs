use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use super::validate::is_special_md_file;

/// A loaded OKF knowledge bundle (directory of concept markdown files).
#[derive(Debug, Clone)]
pub struct KnowledgeBundle {
    pub root: PathBuf,
    pub concepts: HashMap<PathBuf, Concept>,
}

/// A single OKF concept — one markdown file with YAML frontmatter.
#[derive(Debug, Clone)]
pub struct Concept {
    pub rel_path: PathBuf,
    pub frontmatter: Frontmatter,
    pub content: String,
    pub links: Vec<String>,
}

/// YAML frontmatter. `type` is the only required OKF field.
#[derive(Debug, Clone, Deserialize)]
pub struct Frontmatter {
    #[serde(rename = "type")]
    pub concept_type: String,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

impl KnowledgeBundle {
    /// Load all markdown concepts under `root`, skipping agent/special files.
    pub fn load(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref().to_path_buf();
        let mut concepts = HashMap::new();

        for entry in WalkDir::new(&root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .unwrap_or(path)
                .to_path_buf();
            if is_special_md_file(&rel) {
                continue;
            }
            match Concept::from_file(path, &root) {
                Ok(concept) => {
                    concepts.insert(rel, concept);
                }
                Err(e) => {
                    tracing::debug!(
                        path = %rel.display(),
                        error = %e,
                        "Skipping non-concept markdown during OKF load"
                    );
                }
            }
        }

        Ok(Self { root, concepts })
    }
}

impl Concept {
    pub fn from_file(abs_path: &Path, root: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(abs_path)?;
        let rel_path = abs_path.strip_prefix(root).unwrap_or(abs_path).to_path_buf();
        let (frontmatter, body) = parse_frontmatter(&content)?;
        let links = extract_markdown_links(&body);
        Ok(Self {
            rel_path,
            frontmatter,
            content: body,
            links,
        })
    }
}

pub(crate) fn parse_frontmatter(content: &str) -> std::io::Result<(Frontmatter, String)> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing YAML frontmatter",
        ));
    }
    let rest = &trimmed[3..];
    let end = rest.find("\n---").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "unclosed frontmatter")
    })?;
    let yaml_str = &rest[..end];
    let frontmatter: Frontmatter = serde_yaml::from_str(yaml_str).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid frontmatter YAML: {e}"),
        )
    })?;
    let body = rest[end + 4..].trim_start().to_string();
    Ok((frontmatter, body))
}

pub(crate) fn extract_markdown_links(markdown: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\[[^\]]*\]\(([^)]+)\)").expect("link regex");
    re.captures_iter(markdown)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .filter(|dest| !dest.starts_with("http://") && !dest.starts_with("https://"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_links() {
        let md = r#"---
type: knowledge
title: Test
---
# Body

See [other](assets/foo/bar.md) for details.
"#;
        let (fm, body) = parse_frontmatter(md).unwrap();
        assert_eq!(fm.concept_type, "knowledge");
        assert_eq!(fm.title.as_deref(), Some("Test"));
        assert!(body.contains("# Body"));
        let links = extract_markdown_links(&body);
        assert_eq!(links, vec!["assets/foo/bar.md"]);
    }
}

use std::path::{Path, PathBuf};

use super::audit::{scan_index_gaps, IndexGap};
use super::model::KnowledgeBundle;

/// A single bundle validation issue.
#[derive(Debug, Clone)]
pub struct BundleIssue {
    pub path: PathBuf,
    pub reason: String,
}

/// Full validation report for an on-disk OKF knowledge bundle.
#[derive(Debug, Clone, Default)]
pub struct BundleValidationReport {
    pub issues: Vec<BundleIssue>,
}

impl BundleValidationReport {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn format_text(&self) -> String {
        if self.issues.is_empty() {
            return "Knowledge bundle: valid".into();
        }
        let lines: Vec<String> = self
            .issues
            .iter()
            .map(|i| format!("  - {} ({})", i.path.display(), i.reason))
            .collect();
        format!("Knowledge bundle issues:\n{}", lines.join("\n"))
    }
}

/// Validate an on-disk bundle for OKF conformance and internal consistency.
pub fn validate_bundle(root: impl AsRef<Path>) -> BundleValidationReport {
    let root = root.as_ref();
    let mut issues = Vec::new();

    if !root.join("index.md").exists() {
        issues.push(BundleIssue {
            path: root.join("index.md"),
            reason: "bundle entry index.md missing".into(),
        });
    }

    if !root.join("log.md").exists() {
        issues.push(BundleIssue {
            path: root.join("log.md"),
            reason: "bundle changelog log.md missing".into(),
        });
    }

    match KnowledgeBundle::load(root) {
        Ok(bundle) => {
            // Concepts that failed to parse (missing frontmatter) are reported separately.
            collect_unparsed_concepts(root, &bundle, &mut issues);

            for (rel, concept) in &bundle.concepts {
                if concept.frontmatter.concept_type.trim().is_empty() {
                    issues.push(BundleIssue {
                        path: rel.clone(),
                        reason: "OKF frontmatter.type is empty".into(),
                    });
                }
                for link in &concept.links {
                    if let Some(issue) = check_link(root, rel, link) {
                        issues.push(issue);
                    }
                }
            }
        }
        Err(e) => issues.push(BundleIssue {
            path: root.to_path_buf(),
            reason: format!("failed to load bundle: {e}"),
        }),
    }

    for gap in scan_index_gaps(root) {
        issues.push(index_gap_to_issue(gap));
    }

    BundleValidationReport { issues }
}

fn index_gap_to_issue(gap: IndexGap) -> BundleIssue {
    BundleIssue {
        path: gap.path,
        reason: gap.reason,
    }
}

/// Report markdown files that should be OKF concepts but lack valid frontmatter.
fn collect_unparsed_concepts(root: &Path, bundle: &KnowledgeBundle, issues: &mut Vec<BundleIssue>) {
    use walkdir::WalkDir;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        if is_special_md_file(&rel) {
            continue;
        }
        if !bundle.concepts.contains_key(&rel) {
            issues.push(BundleIssue {
                path: rel,
                reason: "missing or invalid OKF frontmatter (type required)".into(),
            });
        }
    }
}

fn check_link(root: &Path, from: &Path, link: &str) -> Option<BundleIssue> {
    let link = link.trim();
    if link.is_empty() || link.starts_with('#') {
        return None;
    }
    let base = from.parent().unwrap_or(Path::new(""));
    let resolved = normalize_link(base, link);
    let target = root.join(&resolved);
    if target.exists() {
        return None;
    }
    Some(BundleIssue {
        path: from.to_path_buf(),
        reason: format!("broken link to '{link}' (resolved: {})", target.display()),
    })
}

fn normalize_link(base: &Path, link: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for part in link.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out
}

/// Files excluded from OKF concept validation (agent plumbing or navigation).
pub(crate) fn is_special_md_file(rel: &Path) -> bool {
    let name = rel
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if matches!(name, "SKILL.md" | "AGENT.md" | "index.md" | "log.md" | "INDEX.md") {
        return true;
    }
    name.ends_with(".archived.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_missing_type_and_broken_link() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("references")).unwrap();
        fs::write(
            dir.path().join("references/BAD.md"),
            "# no frontmatter\n\n[link](missing.md)\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("references/GOOD.md"),
            "---\ntype: user\ntitle: u\n---\n\nok\n",
        )
        .unwrap();

        let report = validate_bundle(dir.path());
        assert!(!report.is_valid());
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.reason.contains("frontmatter") || i.reason.contains("index.md"))
        );
    }
}

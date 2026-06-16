use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::config::{RedactConfig, ValidationConfig};
use crate::frontmatter;
use crate::ops::redact::{RedactionMatch, RedactionReport, redact_body};
use crate::type_registry::SpaceTypeRegistry;

/// Normalize line endings: CRLF → LF, lone CR → LF.
pub fn normalize_line_endings(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

/// Options controlling an ingest run.
#[derive(Debug, Clone, Default)]
pub struct IngestOptions {
    /// Validate only — do not write to disk.
    pub dry_run: bool,
    /// When `Some`, run redaction pass on each file body before validation.
    pub redact: Option<RedactConfig>,
}

/// Result of an ingest operation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IngestReport {
    /// Number of Markdown pages that passed validation.
    pub pages_validated: usize,
    /// Number of non-Markdown asset files discovered.
    pub assets_found: usize,
    /// Validation warning messages (non-fatal).
    pub warnings: Vec<String>,
    /// Redaction reports for any files that had secrets removed.
    #[serde(default)]
    pub redacted: Vec<RedactionReport>,
}

/// Walk `path` (file or directory), validate, optionally redact, and return a report.
pub fn ingest(
    path: &Path,
    options: &IngestOptions,
    wiki_root: &Path,
    registry: &SpaceTypeRegistry,
    validation: &ValidationConfig,
) -> Result<IngestReport> {
    let full_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        wiki_root.join(path)
    };

    if !full_path.exists() {
        bail!("path does not exist: {}", full_path.display());
    }

    // Reject path traversal
    let canonical = full_path.canonicalize()?;
    let canonical_root = wiki_root.canonicalize()?;
    if !canonical.starts_with(&canonical_root) {
        bail!("path is outside wiki root");
    }

    let mut report = IngestReport::default();

    if full_path.is_file() {
        validate_file(
            &full_path,
            wiki_root,
            registry,
            validation,
            options.redact.as_ref(),
            &mut report,
        )?;
    } else {
        for entry in WalkDir::new(&full_path).into_iter().filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_file() {
                if p.extension().and_then(|e| e.to_str()) == Some("md") {
                    validate_file(
                        p,
                        wiki_root,
                        registry,
                        validation,
                        options.redact.as_ref(),
                        &mut report,
                    )?;
                } else {
                    report.assets_found += 1;
                }
            }
        }
    }

    Ok(report)
}

fn slug_from_path(abs_path: &Path, wiki_root: &Path) -> String {
    abs_path
        .strip_prefix(wiki_root)
        .unwrap_or(abs_path)
        .with_extension("")
        .to_string_lossy()
        .into_owned()
}

fn validate_file(
    path: &Path,
    wiki_root: &Path,
    registry: &SpaceTypeRegistry,
    validation: &ValidationConfig,
    redact_cfg: Option<&RedactConfig>,
    report: &mut IngestReport,
) -> Result<()> {
    let raw = std::fs::read_to_string(path)?;
    let mut content = normalize_line_endings(&raw);

    // Redaction pass — body only, before validation
    if let Some(cfg) = redact_cfg {
        let parsed = frontmatter::parse(&content);
        let separator = "---";
        // Find where body starts (after the closing frontmatter delimiter)
        let body_start = if content.starts_with(separator) {
            // skip first "---", find closing "---"
            let after_open = &content[3..];
            after_open
                .find("\n---")
                .map(|pos| 3 + pos + 4 + 1)
                .unwrap_or(0)
        } else {
            0
        };

        if body_start > 0 && body_start <= content.len() {
            let front = &content[..body_start];
            let body = &content[body_start..];
            let (redacted_body, matches) = redact_body(body, cfg);
            if !matches.is_empty() {
                let slug = slug_from_path(path, wiki_root);
                // Adjust line numbers by frontmatter line count
                let fm_lines = front.lines().count();
                let adjusted: Vec<RedactionMatch> = matches
                    .into_iter()
                    .map(|m| RedactionMatch {
                        pattern_name: m.pattern_name,
                        line_number: m.line_number + fm_lines,
                    })
                    .collect();
                report.redacted.push(RedactionReport {
                    slug,
                    matches: adjusted,
                });
                std::fs::write(path, format!("{front}{redacted_body}"))?;
                content = normalize_line_endings(&std::fs::read_to_string(path)?);
            }
        } else {
            // No frontmatter — redact the whole file
            let (redacted, matches) = redact_body(&content, cfg);
            if !matches.is_empty() {
                let slug = slug_from_path(path, wiki_root);
                report.redacted.push(RedactionReport { slug, matches });
                std::fs::write(path, &redacted)?;
                content = normalize_line_endings(&redacted);
            }
        }
        let _ = parsed; // parsed only used to determine frontmatter presence above
    }

    let page = frontmatter::parse(&content);

    // No frontmatter — warn but count as validated
    if page.frontmatter.is_empty() {
        report
            .warnings
            .push(format!("{}: no frontmatter found", path.display()));
        report.pages_validated += 1;
        return Ok(());
    }

    // Validate base fields via type registry
    let warnings = registry.validate(&page.frontmatter, &validation.type_strictness)?;
    for w in warnings {
        report.warnings.push(format!("{}: {}", path.display(), w));
    }

    report.pages_validated += 1;
    Ok(())
}

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use tantivy::{
    Searcher, Term,
    query::TermQuery,
    schema::{IndexRecordOption, Value},
};

use crate::config;
use crate::engine::EngineState;
use crate::index_schema::IndexSchema;
use crate::markdown;
use crate::slug::{ReadTarget, Slug, WikiUri, resolve_read_target};

/// A page that links to a given target — slug and display title.
#[derive(Debug, Clone, Serialize)]
pub struct BacklinkRef {
    /// Slug of the linking page.
    pub slug: String,
    /// Title of the linking page.
    pub title: String,
}

/// Query the index for all pages that contain a link to `target_slug`.
pub fn backlinks_query(
    searcher: &Searcher,
    is: &IndexSchema,
    target_slug: &str,
) -> Result<Vec<BacklinkRef>> {
    let f_body_links = is.field("body_links");
    let f_slug = is.field("slug");
    let f_title = is.field("title");

    let term = Term::from_field_text(f_body_links, target_slug);
    let query = TermQuery::new(term, IndexRecordOption::Basic);

    let doc_addrs = searcher.search(&query, &tantivy::collector::DocSetCollector)?;

    let mut refs: Vec<BacklinkRef> = doc_addrs
        .into_iter()
        .filter_map(|addr| {
            let doc: tantivy::TantivyDocument = searcher.doc(addr).ok()?;
            let slug = doc
                .get_first(f_slug)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let title = doc
                .get_first(f_title)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if slug.is_empty() {
                None
            } else {
                Some(BacklinkRef { slug, title })
            }
        })
        .collect();

    refs.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(refs)
}

/// Return all pages linking to `target_slug` in the named wiki.
pub fn backlinks_for(
    engine: &EngineState,
    wiki_name: &str,
    target_slug: &str,
) -> Result<Vec<BacklinkRef>> {
    let space = engine.space(wiki_name)?;
    let searcher = space.index_manager.searcher()?;
    backlinks_query(&searcher, &space.index_schema, target_slug)
}

/// Result of a content read — page text, asset list, or binary asset.
pub enum ContentReadResult {
    /// Page markdown content (possibly with frontmatter stripped).
    Page(String),
    /// List of co-located asset filenames.
    Assets(Vec<String>),
    /// The resolved target is a binary file — read it directly from disk.
    Binary,
}

/// Read a wiki page or list its co-located assets.
pub fn content_read(
    engine: &EngineState,
    uri: &str,
    wiki_flag: Option<&str>,
    no_frontmatter: bool,
    list_assets: bool,
) -> Result<ContentReadResult> {
    let (entry, slug) = WikiUri::resolve(uri, wiki_flag, &engine.config)?;
    let wiki_root = engine.space(&entry.name)?.wiki_root.clone();

    if list_assets {
        let assets = markdown::list_assets(&slug, &wiki_root)?;
        return Ok(ContentReadResult::Assets(assets));
    }

    match resolve_read_target(slug.as_str(), &wiki_root)? {
        ReadTarget::Page(_) => {
            let wiki_cfg = config::load_wiki(&PathBuf::from(&entry.path)).unwrap_or_default();
            let resolved = config::resolve(&engine.config, &wiki_cfg);
            let strip = no_frontmatter || resolved.read.no_frontmatter;
            let content = markdown::read_page(&slug, &wiki_root, strip)?;
            Ok(ContentReadResult::Page(content))
        }
        ReadTarget::Asset(parent_slug, filename) => {
            let parent = Slug::try_from(parent_slug.as_str())?;
            let bytes = markdown::read_asset(&parent, &filename, &wiki_root)?;
            match String::from_utf8(bytes) {
                Ok(text) => Ok(ContentReadResult::Page(text)),
                Err(_) => Ok(ContentReadResult::Binary),
            }
        }
    }
}

/// Result of a content write operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WriteResult {
    /// Number of bytes written to disk.
    pub bytes_written: usize,
    /// Absolute path of the written file.
    pub path: PathBuf,
}

/// Write content to a wiki page identified by slug or URI.
pub fn content_write(
    engine: &EngineState,
    uri: &str,
    wiki_flag: Option<&str>,
    content: &str,
) -> Result<WriteResult> {
    let (_entry, slug) = WikiUri::resolve(uri, wiki_flag, &engine.config)?;
    let wiki_root = engine.space(&_entry.name)?.wiki_root.clone();
    let path = markdown::write_page(slug.as_str(), content, &wiki_root)?;
    Ok(WriteResult {
        bytes_written: content.len(),
        path,
    })
}

/// v2: 写入门控结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct GatedWriteResult {
    /// 门控决策。
    pub decision: crate::gate::GateDecision,
    /// 写入结果（仅当 Accept 或 NeedsReview 时存在）。
    pub write: Option<WriteResult>,
}

impl GatedWriteResult {
    /// 是否实际写入了文件。
    pub fn written(&self) -> bool {
        self.write.is_some()
    }
}

/// v2: 带门控的写入 —— 在写入前评估内容质量、检测冲突、过滤低价值信息。
///
/// - `Accept`：直接写入。
/// - `NeedsReview`：写入但返回审查标记（调用方可将页面 status 设为 stub）。
/// - `Reject`：不写入，返回原因。
///
/// `gate_config` 为 None 时使用默认配置。
pub fn content_write_gated(
    engine: &EngineState,
    uri: &str,
    wiki_flag: Option<&str>,
    content: &str,
    gate_config: Option<&crate::gate::GateConfig>,
) -> Result<GatedWriteResult> {
    let (entry, slug) = WikiUri::resolve(uri, wiki_flag, &engine.config)?;
    let space = engine.space(&entry.name)?;
    let wiki_root = space.wiki_root.clone();

    // 收集已有 slug 列表用于重复检测
    let existing_slugs: Vec<String> = collect_existing_slugs(&wiki_root)?;

    let config = gate_config.cloned().unwrap_or_default();
    let gate_ctx = crate::gate::GateContext {
        content,
        slug: slug.as_str(),
        existing_slugs: &existing_slugs,
        config: &config,
    };

    let decision = crate::gate::evaluate(&gate_ctx);

    match &decision {
        crate::gate::GateDecision::Accept | crate::gate::GateDecision::NeedsReview(_) => {
            let path = markdown::write_page(slug.as_str(), content, &wiki_root)?;
            Ok(GatedWriteResult {
                decision,
                write: Some(WriteResult {
                    bytes_written: content.len(),
                    path,
                }),
            })
        }
        crate::gate::GateDecision::Reject(_) => Ok(GatedWriteResult {
            decision,
            write: None,
        }),
    }
}

/// 递归收集 wiki_root 下所有 .md 文件的 slug。
fn collect_existing_slugs(wiki_root: &std::path::Path) -> Result<Vec<String>> {
    let mut slugs = Vec::new();
    collect_slugs_inner(wiki_root, wiki_root, &mut slugs)?;
    Ok(slugs)
}

fn collect_slugs_inner(
    base: &std::path::Path,
    dir: &std::path::Path,
    slugs: &mut Vec<String>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_slugs_inner(base, &path, slugs)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(rel) = path.strip_prefix(base) {
                let slug = rel
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                if slug != "index" {
                    slugs.push(slug);
                }
            }
        }
    }
    Ok(())
}

/// Result of creating a new wiki page or section.
pub struct ContentNewResult {
    /// `wiki://` URI for the created page.
    pub uri: String,
    /// Slug of the created page.
    pub slug: String,
    /// Absolute filesystem path of the created file.
    pub path: PathBuf,
    /// Absolute path to the wiki root directory.
    pub wiki_root: PathBuf,
    /// True if the page was created as a bundle (folder + index.md).
    pub bundle: bool,
}

/// Create a new wiki page or section with scaffolded frontmatter.
pub fn content_new(
    engine: &EngineState,
    uri: &str,
    wiki_flag: Option<&str>,
    section: bool,
    bundle: bool,
    name: Option<&str>,
    type_: Option<&str>,
) -> Result<ContentNewResult> {
    let (entry, slug) = WikiUri::resolve(uri, wiki_flag, &engine.config)?;
    let repo_root = PathBuf::from(&entry.path);
    let wiki_root = engine.space(&entry.name)?.wiki_root.clone();

    let type_name = if section {
        "section"
    } else {
        type_.unwrap_or("page")
    };
    let body_template = resolve_body_template(&repo_root, type_name);

    let path = if section {
        markdown::create_section(&slug, &wiki_root, body_template.as_deref())?
    } else {
        markdown::create_page(
            &slug,
            bundle,
            &wiki_root,
            name,
            type_,
            body_template.as_deref(),
        )?
    };

    Ok(ContentNewResult {
        uri: format!("wiki://{}/{slug}", entry.name),
        slug: slug.as_str().to_string(),
        path,
        wiki_root,
        bundle,
    })
}

/// Resolve a body template for a type.
/// 1. `schemas/<type>.md` in the wiki repo
/// 2. Embedded default template
/// 3. None
fn resolve_body_template(repo_root: &Path, type_name: &str) -> Option<String> {
    let template_path = repo_root.join("schemas").join(format!("{type_name}.md"));
    if template_path.is_file() {
        return std::fs::read_to_string(&template_path).ok();
    }
    crate::default_schemas::embedded_body_template(type_name).map(|s| s.to_string())
}

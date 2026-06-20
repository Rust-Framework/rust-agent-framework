use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── Section structs ───────────────────────────────────────────────────────────

/// The `[global]` section of the global config file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalSection {
    /// Name of the wiki used when no `--wiki` flag is given.
    #[serde(default)]
    pub default_wiki: String,
}

/// A registered wiki entry in the `[[wikis]]` array of the global config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiEntry {
    /// Short identifier used in `wiki://` URIs and the `--wiki` flag.
    pub name: String,
    /// Absolute path to the wiki repository root on disk.
    pub path: String,
    /// Optional one-line description shown in `spaces list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional git remote URL for the wiki repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
}

/// Default values for CLI flags that can be overridden per-wiki via `wiki.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    /// Maximum number of search results returned (default: 10).
    #[serde(default = "default_search_top_k")]
    pub search_top_k: u32,
    /// Whether to include BM25 excerpts in search output (default: true).
    #[serde(default = "default_true")]
    pub search_excerpt: bool,
    /// Whether to include section index pages in search results (default: false).
    #[serde(default)]
    pub search_sections: bool,
    /// Page display mode: `"flat"` or `"hierarchical"` (default: `"flat"`).
    #[serde(default = "default_page_mode")]
    pub page_mode: String,
    /// Number of pages returned per `list` call (default: 20).
    #[serde(default = "default_list_page_size")]
    pub list_page_size: u32,
    /// Default output format: `"text"` or `"json"` (default: `"text"`).
    #[serde(default = "default_output_format")]
    pub output_format: String,
    /// Maximum number of tag facet values to return (default: 10).
    #[serde(default = "default_facets_top_tags")]
    pub facets_top_tags: u32,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            search_top_k: 10,
            search_excerpt: true,
            search_sections: false,
            page_mode: "flat".into(),
            list_page_size: 20,
            output_format: "text".into(),
            facets_top_tags: 10,
        }
    }
}

/// `[read]` section — controls how pages are read back.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReadConfig {
    /// Strip frontmatter from `content read` output when true (default: false).
    #[serde(default)]
    pub no_frontmatter: bool,
}

/// `[index]` section — Tantivy index configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Automatically rebuild the index on startup when stale (default: false).
    #[serde(default)]
    pub auto_rebuild: bool,
    /// Automatically recover a corrupt index by rebuilding (default: true).
    #[serde(default = "default_true")]
    pub auto_recovery: bool,
    /// Tantivy index writer memory budget in megabytes (default: 50).
    #[serde(default = "default_memory_budget_mb")]
    pub memory_budget_mb: u32,
    /// Tantivy tokenizer name (default: `"en_stem"`).
    #[serde(default = "default_tokenizer")]
    pub tokenizer: String,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            auto_rebuild: false,
            auto_recovery: true,
            memory_budget_mb: 50,
            tokenizer: "en_stem".into(),
        }
    }
}

/// Graph rendering and community detection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConfig {
    /// Default graph output format: `"mermaid"`, `"dot"`, or `"llms"` (default: `"mermaid"`).
    #[serde(default = "default_graph_format")]
    pub format: String,
    /// Default hop depth for subgraph extraction (default: 3).
    #[serde(default = "default_graph_depth")]
    pub depth: u32,
    /// Page types to include when no `--type` flag is given (empty = all).
    #[serde(default)]
    pub r#type: Vec<String>,
    /// Default output file path for graph commands (empty = stdout).
    #[serde(default)]
    pub output: String,
    /// Minimum local node count before Louvain community detection runs (default 30).
    #[serde(default = "default_min_nodes_for_communities")]
    pub min_nodes_for_communities: usize,
    /// Maximum community-peer suggestions returned by `wiki_suggest` strategy 4 (default 2).
    #[serde(default = "default_community_suggestions_limit")]
    pub community_suggestions_limit: usize,
    /// Enable snapshot warm-start for the graph cache (default: true).
    /// Set false in CI or tests to avoid snapshot files.
    #[serde(default = "default_true")]
    pub snapshot: bool,
    /// Number of snapshots to retain per wiki space (default: 3).
    #[serde(default = "default_snapshot_keep")]
    pub snapshot_keep: u32,
    /// Snapshot format: "bincode+lz4" | "bincode" | "bincode+zstd" (default: "bincode+lz4").
    #[serde(default = "default_snapshot_format")]
    pub snapshot_format: String,
    /// Enable structural topology algorithms in wiki_stats (diameter, radius, center).
    /// Lint rules articulation-point, bridge, periphery are always available via --rules.
    /// Default: true. Set false to skip structural computation in stats entirely.
    #[serde(default = "default_true")]
    pub structural_algorithms: bool,
    /// Maximum local node count before O(n²) diameter/radius/center/periphery algorithms are skipped (default: 2000).
    #[serde(default = "default_max_nodes_for_diameter")]
    pub max_nodes_for_diameter: usize,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            format: "mermaid".into(),
            depth: 3,
            r#type: Vec::new(),
            output: String::new(),
            min_nodes_for_communities: default_min_nodes_for_communities(),
            community_suggestions_limit: default_community_suggestions_limit(),
            snapshot: true,
            snapshot_keep: 3,
            snapshot_format: "bincode+lz4".into(),
            structural_algorithms: true,
            max_nodes_for_diameter: default_max_nodes_for_diameter(),
        }
    }
}

fn default_min_nodes_for_communities() -> usize {
    30
}

fn default_community_suggestions_limit() -> usize {
    2
}

fn default_snapshot_keep() -> u32 {
    3
}

fn default_snapshot_format() -> String {
    "bincode+lz4".into()
}
fn default_max_nodes_for_diameter() -> usize {
    2000
}


/// `[validation]` section — frontmatter validation strictness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// How strictly unknown types are treated: `"loose"` (warn) or `"strict"` (error) (default: `"loose"`).
    #[serde(default = "default_type_strictness")]
    pub type_strictness: String,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            type_strictness: "loose".into(),
        }
    }
}


/// `[watch]` section — filesystem watcher configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Debounce delay in milliseconds before triggering ingest after a file change (default: 500).
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u32,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self { debounce_ms: 500 }
    }
}

/// `[suggest]` section — related-page suggestion defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestConfig {
    /// Default maximum number of suggestions to return (default: 5).
    #[serde(default = "default_suggest_limit")]
    pub default_limit: u32,
    /// Minimum relevance score for a suggestion to be included (default: 0.1).
    #[serde(default = "default_suggest_min_score")]
    pub min_score: f32,
}

impl Default for SuggestConfig {
    fn default() -> Self {
        Self {
            default_limit: 5,
            min_score: 0.1,
        }
    }
}

/// `[search]` section — BM25 score multipliers by page status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Map of status value → score multiplier applied to BM25 results.
    #[serde(default = "default_search_status")]
    pub status: std::collections::HashMap<String, f32>,
}

fn default_search_status() -> std::collections::HashMap<String, f32> {
    [
        ("active".into(), 1.0_f32),
        ("draft".into(), 0.8),
        ("archived".into(), 0.3),
        ("unknown".into(), 0.9),
    ]
    .into_iter()
    .collect()
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            status: default_search_status(),
        }
    }
}

/// Configuration for the `stale` lint rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LintConfig {
    /// Pages not updated within this many days are candidates for the `stale` rule (default 90).
    #[serde(default = "default_stale_days")]
    pub stale_days: u32,
    /// `stale` only fires when `confidence` is also below this threshold (default 0.4).
    #[serde(default = "default_stale_confidence_threshold")]
    pub stale_confidence_threshold: f32,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            stale_days: default_stale_days(),
            stale_confidence_threshold: default_stale_confidence_threshold(),
        }
    }
}

/// A user-defined redaction rule added to the built-in patterns.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomPattern {
    /// Unique name used in redaction reports.
    pub name: String,
    /// Regex pattern to match sensitive text.
    pub pattern: String,
    /// Replacement string substituted for matched text (e.g. `"[REDACTED]"`).
    pub replacement: String,
}

/// `[redact]` section — sensitive-data redaction configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RedactConfig {
    /// Built-in pattern names to disable (e.g. `["aws-key"]`).
    #[serde(default)]
    pub disable: Vec<String>,
    /// Additional user-defined redaction patterns.
    #[serde(default)]
    pub patterns: Vec<CustomPattern>,
}

/// `[decay]` section — v2 forgetting-curve configuration.
pub type DecayConfig = crate::forgetting::DecayConfig;
/// `[gate]` section — v2 write-gating configuration.
pub type GateConfig = crate::gate::GateConfig;
/// `[governance]` section — v2 automated-governance configuration.
pub type GovernanceConfig = crate::governance::GovernanceConfig;
/// `[memory]` section — v2 layered-memory configuration.
pub type MemoryConfig = crate::memory::MemoryConfig;

// ── Composite configs ─────────────────────────────────────────────────────────

/// Root structure for `~/.llm-wiki/config.toml` — the global configuration file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    /// `[global]` section.
    #[serde(default)]
    pub global: GlobalSection,
    /// `[[wikis]]` array — registered wiki spaces.
    #[serde(default)]
    pub wikis: Vec<WikiEntry>,
    /// `[defaults]` section — CLI flag defaults.
    #[serde(default)]
    pub defaults: Defaults,
    /// `[read]` section.
    #[serde(default)]
    pub read: ReadConfig,
    /// `[index]` section.
    #[serde(default)]
    pub index: IndexConfig,
    /// `[graph]` section.
    #[serde(default)]
    pub graph: GraphConfig,
    /// `[validation]` section.
    #[serde(default)]
    pub validation: ValidationConfig,
    /// `[suggest]` section.
    #[serde(default)]
    pub suggest: SuggestConfig,
    /// `[search]` section.
    #[serde(default)]
    pub search: SearchConfig,
    /// `[lint]` section.
    #[serde(default)]
    pub lint: LintConfig,
    /// `[watch]` section.
    #[serde(default)]
    pub watch: WatchConfig,
    /// `[redact]` section.
    #[serde(default)]
    pub redact: RedactConfig,
    /// `[decay]` section — v2 forgetting curve.
    #[serde(default)]
    pub decay: DecayConfig,
    /// `[gate]` section — v2 write gating.
    #[serde(default)]
    pub gate: GateConfig,
    /// `[governance]` section — v2 automated governance.
    #[serde(default)]
    pub governance: GovernanceConfig,
    /// `[memory]` section — v2 layered memory.
    #[serde(default)]
    pub memory: MemoryConfig,
}

/// A type entry in `[types.<name>]` of `wiki.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeEntry {
    /// Relative path to the JSON Schema file for this type.
    pub schema: String,
    /// Human-readable description of the type.
    pub description: String,
}

/// Per-wiki configuration loaded from `<wiki-root>/wiki.toml`.
///
/// Fields present here override the corresponding `GlobalConfig` sections.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WikiConfig {
    /// Wiki display name (informational; used in export headers).
    #[serde(default)]
    pub name: String,
    /// One-line description of the wiki.
    #[serde(default)]
    pub description: String,
    /// `[types.<name>]` custom type registrations for this wiki.
    #[serde(default)]
    pub types: std::collections::HashMap<String, TypeEntry>,
    /// Per-wiki override for `[defaults]`.
    #[serde(default)]
    pub defaults: Option<Defaults>,
    /// Per-wiki override for `[read]`.
    #[serde(default)]
    pub read: Option<ReadConfig>,
    /// Per-wiki override for `[validation]`.
    #[serde(default)]
    pub validation: Option<ValidationConfig>,
    /// Per-wiki override for `[graph]`.
    #[serde(default)]
    pub graph: Option<GraphConfig>,
    /// Per-wiki override for `[suggest]`.
    #[serde(default)]
    pub suggest: Option<SuggestConfig>,
    /// Per-wiki override for `[search]`.
    #[serde(default)]
    pub search: Option<SearchConfig>,
    /// Per-wiki override for `[lint]`.
    #[serde(default)]
    pub lint: Option<LintConfig>,
    /// Per-wiki override for `[redact]`.
    #[serde(default)]
    pub redact: Option<RedactConfig>,
    /// Per-wiki override for `[decay]` (v2 forgetting curve).
    #[serde(default)]
    pub decay: Option<DecayConfig>,
    /// Per-wiki override for `[gate]` (v2 write gating).
    #[serde(default)]
    pub gate: Option<GateConfig>,
    /// Per-wiki override for `[governance]` (v2 automated governance).
    #[serde(default)]
    pub governance: Option<GovernanceConfig>,
    /// Per-wiki override for `[memory]` (v2 layered memory).
    #[serde(default)]
    pub memory: Option<MemoryConfig>,
    /// Content directory relative to repo root. Default: `"wiki"`.
    #[serde(default = "default_wiki_root")]
    pub wiki_root: String,
}

/// Fully merged config for a specific wiki — global settings overlaid with per-wiki overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedConfig {
    /// Resolved defaults section.
    pub defaults: Defaults,
    /// Resolved read section.
    pub read: ReadConfig,
    /// Resolved index section (always from global).
    pub index: IndexConfig,
    /// Resolved graph section.
    pub graph: GraphConfig,
    /// Resolved validation section.
    pub validation: ValidationConfig,
    /// Resolved suggest section.
    pub suggest: SuggestConfig,
    /// Resolved search section (merged: per-wiki entries override global entries).
    pub search: SearchConfig,
    /// Resolved lint section.
    pub lint: LintConfig,
    /// Resolved redact section.
    pub redact: RedactConfig,
    /// Resolved decay section (v2 forgetting curve).
    pub decay: DecayConfig,
    /// Resolved gate section (v2 write gating).
    pub gate: GateConfig,
    /// Resolved governance section (v2 automated governance).
    pub governance: GovernanceConfig,
    /// Resolved memory section (v2 layered memory).
    pub memory: MemoryConfig,
}

// ── Default value helpers ─────────────────────────────────────────────────────

fn default_search_top_k() -> u32 {
    10
}
fn default_true() -> bool {
    true
}
fn default_page_mode() -> String {
    "flat".into()
}
fn default_list_page_size() -> u32 {
    20
}
fn default_output_format() -> String {
    "text".into()
}
fn default_facets_top_tags() -> u32 {
    10
}
fn default_memory_budget_mb() -> u32 {
    50
}
fn default_tokenizer() -> String {
    "en_stem".into()
}
fn default_graph_format() -> String {
    "mermaid".into()
}
fn default_graph_depth() -> u32 {
    3
}
fn default_type_strictness() -> String {
    "loose".into()
}
fn default_debounce_ms() -> u32 {
    500
}
fn default_suggest_limit() -> u32 {
    5
}
fn default_suggest_min_score() -> f32 {
    0.1
}
fn default_stale_days() -> u32 {
    90
}
fn default_stale_confidence_threshold() -> f32 {
    0.4
}
fn default_wiki_root() -> String {
    "wiki".to_string()
}
// ── Functions ─────────────────────────────────────────────────────────────────

/// Merge global and per-wiki config into a `ResolvedConfig` for a specific wiki.
pub fn resolve(global: &GlobalConfig, per_wiki: &WikiConfig) -> ResolvedConfig {
    ResolvedConfig {
        defaults: per_wiki
            .defaults
            .clone()
            .unwrap_or_else(|| global.defaults.clone()),
        read: per_wiki.read.clone().unwrap_or_else(|| global.read.clone()),
        index: global.index.clone(),
        graph: per_wiki
            .graph
            .clone()
            .unwrap_or_else(|| global.graph.clone()),
        validation: per_wiki
            .validation
            .clone()
            .unwrap_or_else(|| global.validation.clone()),
        suggest: per_wiki
            .suggest
            .clone()
            .unwrap_or_else(|| global.suggest.clone()),
        search: {
            let mut merged = global.search.status.clone();
            if let Some(wiki_search) = &per_wiki.search {
                for (k, v) in &wiki_search.status {
                    merged.insert(k.clone(), *v);
                }
            }
            SearchConfig { status: merged }
        },
        lint: per_wiki.lint.clone().unwrap_or_else(|| global.lint.clone()),
        redact: per_wiki
            .redact
            .clone()
            .unwrap_or_else(|| global.redact.clone()),
        decay: per_wiki
            .decay
            .clone()
            .unwrap_or_else(|| global.decay.clone()),
        gate: per_wiki.gate.clone().unwrap_or_else(|| global.gate.clone()),
        governance: per_wiki
            .governance
            .clone()
            .unwrap_or_else(|| global.governance.clone()),
        memory: per_wiki
            .memory
            .clone()
            .unwrap_or_else(|| global.memory.clone()),
    }
}

/// Load the global config from a TOML file. Returns default config if the file is absent.
pub fn load_global(path: &Path) -> Result<GlobalConfig> {
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config: GlobalConfig =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config)
}

/// Load per-wiki config from `<wiki_root>/wiki.toml`. Returns default config if absent.
pub fn load_wiki(wiki_root: &Path) -> Result<WikiConfig> {
    let path = wiki_root.join("wiki.toml");
    if !path.exists() {
        return Ok(WikiConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config: WikiConfig =
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config)
}

/// Serialize and write the global config to `path`, creating parent dirs if needed.
pub fn save_global(config: &GlobalConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Serialize and write the per-wiki config to `<wiki_root>/wiki.toml`.
pub fn save_wiki(config: &WikiConfig, wiki_root: &Path) -> Result<()> {
    let path = wiki_root.join("wiki.toml");
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Set a dot-notation config key on a `GlobalConfig` in place. Errors on unknown keys.
pub fn set_global_config_value(global: &mut GlobalConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "global.default_wiki" => global.global.default_wiki = value.into(),
        "defaults.search_top_k" => global.defaults.search_top_k = value.parse()?,
        "defaults.search_excerpt" => global.defaults.search_excerpt = value.parse()?,
        "defaults.search_sections" => global.defaults.search_sections = value.parse()?,
        "defaults.page_mode" => global.defaults.page_mode = value.into(),
        "defaults.list_page_size" => global.defaults.list_page_size = value.parse()?,
        "defaults.output_format" => global.defaults.output_format = value.into(),
        "defaults.facets_top_tags" => global.defaults.facets_top_tags = value.parse()?,
        "read.no_frontmatter" => global.read.no_frontmatter = value.parse()?,
        "index.auto_rebuild" => global.index.auto_rebuild = value.parse()?,
        "index.auto_recovery" => global.index.auto_recovery = value.parse()?,
        "index.memory_budget_mb" => global.index.memory_budget_mb = value.parse()?,
        "index.tokenizer" => global.index.tokenizer = value.into(),
        "graph.format" => global.graph.format = value.into(),
        "graph.depth" => global.graph.depth = value.parse()?,
        "graph.output" => global.graph.output = value.into(),
        "graph.snapshot" => global.graph.snapshot = value.parse()?,
        "graph.snapshot_keep" => global.graph.snapshot_keep = value.parse()?,
        "graph.snapshot_format" => global.graph.snapshot_format = value.into(),
        "graph.structural_algorithms" => global.graph.structural_algorithms = value.parse()?,
        "graph.max_nodes_for_diameter" => global.graph.max_nodes_for_diameter = value.parse()?,
        "suggest.default_limit" => global.suggest.default_limit = value.parse()?,
        "suggest.min_score" => global.suggest.min_score = value.parse()?,
        "validation.type_strictness" => global.validation.type_strictness = value.into(),
        "watch.debounce_ms" => global.watch.debounce_ms = value.parse()?,
        _ => anyhow::bail!("unknown key: {key}"),
    }
    Ok(())
}

/// Read a dot-notation config key from `ResolvedConfig`/`GlobalConfig`. Returns `"unknown key"` for unrecognized keys.
pub fn get_config_value(resolved: &ResolvedConfig, global: &GlobalConfig, key: &str) -> String {
    match key {
        "global.default_wiki" => global.global.default_wiki.clone(),
        "defaults.search_top_k" => resolved.defaults.search_top_k.to_string(),
        "defaults.search_excerpt" => resolved.defaults.search_excerpt.to_string(),
        "defaults.search_sections" => resolved.defaults.search_sections.to_string(),
        "defaults.page_mode" => resolved.defaults.page_mode.clone(),
        "defaults.list_page_size" => resolved.defaults.list_page_size.to_string(),
        "defaults.output_format" => resolved.defaults.output_format.clone(),
        "defaults.facets_top_tags" => resolved.defaults.facets_top_tags.to_string(),
        "read.no_frontmatter" => resolved.read.no_frontmatter.to_string(),
        "index.auto_rebuild" => resolved.index.auto_rebuild.to_string(),
        "index.auto_recovery" => global.index.auto_recovery.to_string(),
        "index.memory_budget_mb" => global.index.memory_budget_mb.to_string(),
        "index.tokenizer" => global.index.tokenizer.clone(),
        "graph.format" => resolved.graph.format.clone(),
        "graph.depth" => resolved.graph.depth.to_string(),
        "graph.output" => resolved.graph.output.clone(),
        "graph.snapshot" => resolved.graph.snapshot.to_string(),
        "graph.snapshot_keep" => resolved.graph.snapshot_keep.to_string(),
        "graph.snapshot_format" => resolved.graph.snapshot_format.clone(),
        "graph.structural_algorithms" => resolved.graph.structural_algorithms.to_string(),
        "graph.max_nodes_for_diameter" => resolved.graph.max_nodes_for_diameter.to_string(),
        "validation.type_strictness" => resolved.validation.type_strictness.clone(),
        "watch.debounce_ms" => global.watch.debounce_ms.to_string(),
        "suggest.default_limit" => resolved.suggest.default_limit.to_string(),
        "suggest.min_score" => resolved.suggest.min_score.to_string(),
        _ => format!("unknown key: {key}"),
    }
}

/// Set a dot-notation config key on a `WikiConfig` in place. Errors on global-only or unknown keys.
pub fn set_wiki_config_value(wiki_cfg: &mut WikiConfig, key: &str, value: &str) -> Result<()> {
    match key {
        "defaults.search_top_k" => {
            wiki_cfg
                .defaults
                .get_or_insert_with(Defaults::default)
                .search_top_k = value.parse()?;
        }
        "defaults.search_excerpt" => {
            wiki_cfg
                .defaults
                .get_or_insert_with(Defaults::default)
                .search_excerpt = value.parse()?;
        }
        "defaults.search_sections" => {
            wiki_cfg
                .defaults
                .get_or_insert_with(Defaults::default)
                .search_sections = value.parse()?;
        }
        "defaults.page_mode" => {
            wiki_cfg
                .defaults
                .get_or_insert_with(Defaults::default)
                .page_mode = value.into();
        }
        "defaults.list_page_size" => {
            wiki_cfg
                .defaults
                .get_or_insert_with(Defaults::default)
                .list_page_size = value.parse()?;
        }
        "defaults.output_format" => {
            wiki_cfg
                .defaults
                .get_or_insert_with(Defaults::default)
                .output_format = value.into();
        }
        "defaults.facets_top_tags" => {
            wiki_cfg
                .defaults
                .get_or_insert_with(Defaults::default)
                .facets_top_tags = value.parse()?;
        }
        "read.no_frontmatter" => {
            wiki_cfg
                .read
                .get_or_insert_with(ReadConfig::default)
                .no_frontmatter = value.parse()?;
        }
        "validation.type_strictness" => {
            wiki_cfg
                .validation
                .get_or_insert_with(ValidationConfig::default)
                .type_strictness = value.into();
        }
        "suggest.default_limit" => {
            wiki_cfg
                .suggest
                .get_or_insert_with(SuggestConfig::default)
                .default_limit = value.parse()?;
        }
        "suggest.min_score" => {
            wiki_cfg
                .suggest
                .get_or_insert_with(SuggestConfig::default)
                .min_score = value.parse()?;
        }
        "graph.format" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .format = value.into();
        }
        "graph.depth" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .depth = value.parse()?;
        }
        "graph.output" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .output = value.into();
        }
        "graph.snapshot" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .snapshot = value.parse()?;
        }
        "graph.snapshot_keep" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .snapshot_keep = value.parse()?;
        }
        "graph.snapshot_format" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .snapshot_format = value.into();
        }
        "graph.structural_algorithms" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .structural_algorithms = value.parse()?;
        }
        "graph.max_nodes_for_diameter" => {
            wiki_cfg
                .graph
                .get_or_insert_with(GraphConfig::default)
                .max_nodes_for_diameter = value.parse()?;
        }
        "global.default_wiki"
        | "index.auto_rebuild"
        | "index.auto_recovery"
        | "index.memory_budget_mb"
        | "index.tokenizer" => {
            anyhow::bail!("{key} is a global-only key \u{2014} use --global");
        }
        "watch.debounce_ms" => {
            anyhow::bail!("{key} is a global-only key \u{2014} use --global");
        }
        _ => anyhow::bail!("unknown key: {key}"),
    }
    Ok(())
}

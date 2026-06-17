//! Structured observability for MemoryAgent consolidation runs.
//!
//! All MemoryAgent output is routed through tracing — never to the user REPL.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use uuid::Uuid;

use super::index_audit::IndexGap;

static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Observability verbosity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryObsLevel {
    Prod,
    Dev,
    Trace,
}

impl MemoryObsLevel {
    pub fn from_env() -> Self {
        match std::env::var("MEMORY_OBS_LEVEL")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "trace" => Self::Trace,
            "dev" => Self::Dev,
            _ => Self::Prod,
        }
    }
}

/// Consolidation outcome status for `memory.consolidation` events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsolidationStatus {
    Ok,
    Updated,
    IndexGap,
    Error,
    Skipped,
}

impl ConsolidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Updated => "updated",
            Self::IndexGap => "index_gap",
            Self::Error => "error",
            Self::Skipped => "skipped",
        }
    }
}

/// Parsed result from MemoryAgent LLM output + index audit.
#[derive(Debug, Clone)]
pub struct MemoryConsolidationResult {
    pub status: ConsolidationStatus,
    pub updated_files: Vec<String>,
    pub index_gap_paths: Vec<String>,
    pub raw_output_sample: Option<String>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
}

/// Context passed into a consolidation run for event correlation.
#[derive(Debug, Clone)]
pub struct ConsolidationRunContext {
    pub run_id: String,
    pub session_id: Option<String>,
    pub messages_count: usize,
    pub queue_coalesced_dropped: u64,
    pub started_at: Instant,
}

impl ConsolidationRunContext {
    pub fn new(
        session_id: Option<String>,
        messages_count: usize,
        queue_coalesced_dropped: u64,
    ) -> Self {
        let seq = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            run_id: format!("{}-{}", Uuid::new_v4(), seq),
            session_id,
            messages_count,
            queue_coalesced_dropped,
            started_at: Instant::now(),
        }
    }

    pub fn duration_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

const RAW_SAMPLE_MAX: usize = 200;
const UPDATED_FILES_MAX: usize = 32;

/// Parse raw LLM output into a structured consolidation result.
pub fn parse_memory_output(raw: &str, index_gaps: &[IndexGap]) -> MemoryConsolidationResult {
    let trimmed = raw.trim();
    let mut updated_files = Vec::new();
    let mut llm_index_gap_paths = Vec::new();
    let mut error_kind = None;
    let mut error_message = None;

    let mut status = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("ok") {
        ConsolidationStatus::Ok
    } else if let Some(block) = extract_block(trimmed, "UPDATED:") {
        updated_files = parse_updated_lines(&block);
        if updated_files.is_empty() {
            ConsolidationStatus::Ok
        } else {
            ConsolidationStatus::Updated
        }
    } else if let Some(block) = extract_block(trimmed, "INDEX_GAP:") {
        llm_index_gap_paths = parse_index_gap_lines(&block);
        ConsolidationStatus::IndexGap
    } else if let Some(block) = extract_block(trimmed, "ERROR:") {
        error_kind = Some("llm_reported".into());
        error_message = Some(block);
        ConsolidationStatus::Error
    } else {
        ConsolidationStatus::Ok
    };

    let mut index_gap_paths: Vec<String> = index_gaps
        .iter()
        .map(|g| g.path.display().to_string())
        .collect();

    if index_gap_paths.is_empty() && !llm_index_gap_paths.is_empty() {
        index_gap_paths = llm_index_gap_paths;
    }

    if !index_gap_paths.is_empty() {
        status = ConsolidationStatus::IndexGap;
    }

    let raw_output_sample = if trimmed.len() > RAW_SAMPLE_MAX {
        Some(format!("{}...", &trimmed[..RAW_SAMPLE_MAX]))
    } else if !trimmed.is_empty()
        && matches!(status, ConsolidationStatus::Ok | ConsolidationStatus::Error)
    {
        Some(trimmed.to_string())
    } else {
        None
    };

    MemoryConsolidationResult {
        status,
        updated_files: updated_files.into_iter().take(UPDATED_FILES_MAX).collect(),
        index_gap_paths,
        raw_output_sample,
        error_kind,
        error_message,
    }
}

fn extract_block(text: &str, marker: &str) -> Option<String> {
    let idx = text.find(marker)?;
    Some(text[idx + marker.len()..].trim().to_string())
}

fn parse_updated_lines(block: &str) -> Vec<String> {
    block
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with('-') {
                return None;
            }
            let rest = line.trim_start_matches('-').trim();
            rest.split(':').next().map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_index_gap_lines(block: &str) -> Vec<String> {
    block
        .lines()
        .filter_map(|line| {
            let line = line.trim().trim_start_matches('-').trim();
            if line.is_empty() {
                return None;
            }
            Some(
                line.split('(')
                    .next()
                    .unwrap_or(line)
                    .trim()
                    .to_string(),
            )
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Emit `memory.consolidation` structured event via tracing.
pub fn emit_consolidation_event(
    ctx: &ConsolidationRunContext,
    result: &MemoryConsolidationResult,
    level: MemoryObsLevel,
) {
    let status = result.status.as_str();
    let duration_ms = ctx.duration_ms();
    let updated_files_count = result.updated_files.len();
    let index_gap_count = result.index_gap_paths.len();

    let log_ok_at_info = matches!(level, MemoryObsLevel::Dev | MemoryObsLevel::Trace);

    match result.status {
        ConsolidationStatus::Error => {
            tracing::error!(
                event = "memory.consolidation",
                run_id = %ctx.run_id,
                session_id = ?ctx.session_id,
                status,
                duration_ms,
                messages_count = ctx.messages_count,
                updated_files_count,
                index_gap_count,
                queue_coalesced_dropped = ctx.queue_coalesced_dropped,
                updated_files = ?result.updated_files,
                index_gap_paths = ?result.index_gap_paths,
                error_kind = ?result.error_kind.as_deref().unwrap_or("unknown"),
                error_message = ?result.error_message,
                "Memory consolidation failed"
            );
        }
        ConsolidationStatus::IndexGap => {
            tracing::warn!(
                event = "memory.consolidation",
                run_id = %ctx.run_id,
                session_id = ?ctx.session_id,
                status,
                duration_ms,
                messages_count = ctx.messages_count,
                updated_files_count,
                index_gap_count,
                queue_coalesced_dropped = ctx.queue_coalesced_dropped,
                updated_files = ?result.updated_files,
                index_gap_paths = ?result.index_gap_paths,
                "Memory consolidation index gaps detected"
            );
        }
        ConsolidationStatus::Updated => {
            tracing::info!(
                event = "memory.consolidation",
                run_id = %ctx.run_id,
                session_id = ?ctx.session_id,
                status,
                duration_ms,
                messages_count = ctx.messages_count,
                updated_files_count,
                index_gap_count,
                queue_coalesced_dropped = ctx.queue_coalesced_dropped,
                updated_files = ?result.updated_files,
                index_gap_paths = ?result.index_gap_paths,
                "Memory consolidation updated"
            );
        }
        ConsolidationStatus::Skipped => {
            tracing::debug!(
                event = "memory.consolidation",
                run_id = %ctx.run_id,
                session_id = ?ctx.session_id,
                status,
                duration_ms,
                messages_count = ctx.messages_count,
                queue_coalesced_dropped = ctx.queue_coalesced_dropped,
                "Memory consolidation skipped"
            );
        }
        ConsolidationStatus::Ok => {
            if log_ok_at_info {
                tracing::info!(
                    event = "memory.consolidation",
                    run_id = %ctx.run_id,
                    session_id = ?ctx.session_id,
                    status,
                    duration_ms,
                    messages_count = ctx.messages_count,
                    updated_files_count,
                    index_gap_count,
                    queue_coalesced_dropped = ctx.queue_coalesced_dropped,
                    "Memory consolidation ok"
                );
            } else {
                tracing::debug!(
                    event = "memory.consolidation",
                    run_id = %ctx.run_id,
                    session_id = ?ctx.session_id,
                    status,
                    duration_ms,
                    messages_count = ctx.messages_count,
                    updated_files_count,
                    index_gap_count,
                    queue_coalesced_dropped = ctx.queue_coalesced_dropped,
                    "Memory consolidation ok"
                );
            }
        }
    }

    if let Some(ref sample) = result.raw_output_sample {
        tracing::debug!(
            event = "memory.consolidation.raw_output",
            run_id = %ctx.run_id,
            raw_output_sample = %sample,
            "MemoryAgent raw output sample"
        );
    }
}

pub fn emit_consolidation_error(
    ctx: &ConsolidationRunContext,
    error_kind: &str,
    error_message: &str,
) {
    tracing::error!(
        event = "memory.consolidation",
        run_id = %ctx.run_id,
        session_id = ?ctx.session_id,
        status = "error",
        duration_ms = ctx.duration_ms(),
        messages_count = ctx.messages_count,
        queue_coalesced_dropped = ctx.queue_coalesced_dropped,
        error_kind,
        error_message,
        "Memory consolidation error"
    );
}

pub fn emit_worker_started(ctx: &ConsolidationRunContext) {
    tracing::info!(
        event = "memory.worker.started",
        run_id = %ctx.run_id,
        session_id = ?ctx.session_id,
        messages_count = ctx.messages_count,
        queue_coalesced_dropped = ctx.queue_coalesced_dropped,
        "Memory consolidation worker started job"
    );
}

pub fn emit_worker_finished(ctx: &ConsolidationRunContext, status: &str) {
    tracing::info!(
        event = "memory.worker.finished",
        run_id = %ctx.run_id,
        session_id = ?ctx.session_id,
        status,
        duration_ms = ctx.duration_ms(),
        messages_count = ctx.messages_count,
        queue_coalesced_dropped = ctx.queue_coalesced_dropped,
        "Memory consolidation worker finished job"
    );
}

pub fn emit_worker_coalesced(dropped: u64) {
    tracing::debug!(
        event = "memory.worker.coalesced",
        dropped,
        "Memory consolidation jobs coalesced"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_ok() {
        let r = parse_memory_output("OK", &[]);
        assert_eq!(r.status, ConsolidationStatus::Ok);
        assert!(r.updated_files.is_empty());
    }

    #[test]
    fn parse_updated_with_chatter() {
        let raw = "让我写入记忆...\n\nUPDATED:\n- references/USER.md: 新增\n- references/SOUL.md: 更新";
        let r = parse_memory_output(raw, &[]);
        assert_eq!(r.status, ConsolidationStatus::Updated);
        assert_eq!(r.updated_files.len(), 2);
    }

    #[test]
    fn parse_pure_chatter_defaults_ok() {
        let r = parse_memory_output("今天有什么想聊的？😊", &[]);
        assert_eq!(r.status, ConsolidationStatus::Ok);
        assert!(r.raw_output_sample.is_some());
    }

    #[test]
    fn event_contract_index_gap_has_paths() {
        let gaps = vec![
            IndexGap {
                path: PathBuf::from("assets/INDEX.md"),
                reason: "empty".into(),
            },
            IndexGap {
                path: PathBuf::from("assets/foo/INDEX.md"),
                reason: "missing".into(),
            },
        ];
        let r = parse_memory_output("OK", &gaps);
        assert_eq!(r.status, ConsolidationStatus::IndexGap);
        assert_eq!(r.index_gap_paths.len(), 2);
    }

    #[test]
    fn parse_llm_index_gap_paths() {
        let raw = "INDEX_GAP:\n- assets/INDEX.md (empty but topics exist)\n- assets/foo/INDEX.md (missing)";
        let r = parse_memory_output(raw, &[]);
        assert_eq!(r.status, ConsolidationStatus::IndexGap);
        assert_eq!(r.index_gap_paths.len(), 2);
    }

    #[test]
    fn parse_updated_empty_lines_becomes_ok() {
        let raw = "UPDATED:\n(no files)";
        let r = parse_memory_output(raw, &[]);
        assert_eq!(r.status, ConsolidationStatus::Ok);
    }

    #[test]
    fn event_contract_updated_has_files() {
        let raw = "UPDATED:\n- references/USER.md: 更新\n- references/SOUL.md: 新增";
        let r = parse_memory_output(raw, &[]);
        assert_eq!(r.status, ConsolidationStatus::Updated);
        assert!(!r.updated_files.is_empty());
    }
}

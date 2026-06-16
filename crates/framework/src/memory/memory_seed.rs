//! Memory directory seeding — syncs the runtime memory directory from the
//! built-in template.
//!
//! # Design
//!
//! - `SKILL.md` and `AGENT.md` are **always** guaranteed to exist (system-required).
//! - Data files (`references/*.md`, `assets/INDEX.md`) are compared by content:
//!   if unchanged, the template refreshes them (picks up code-level updates);
//!   if modified by MemoryAgent, user data is preserved.
//!
//! # Usage
//!
//! Call `seed_memory_dir(target)` once during `SkillMemoryContextProvider`
//! construction.  It is idempotent — existing user data is never overwritten.

use std::fs;
use std::path::Path;

/// Built-in template directory, resolved at compile time.
/// Contains SKILL.md, AGENT.md, references/*.md, assets/INDEX.md.
pub const TEMPLATE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/memory/skill");

/// Seed the target memory directory from the built-in template.
///
/// For each file: if the target doesn't exist or its content matches the
/// template exactly, copy it.  If the content differs (MemoryAgent has
/// written user data), skip it to preserve user information.
pub fn seed_memory_dir(target: &Path) {
    let template = Path::new(TEMPLATE_DIR);
    if !template.exists() {
        tracing::warn!("Memory template directory not found: {}", template.display());
        return;
    }
    match sync_dir(template, target) {
        Ok(()) => tracing::info!(
            "Synchronized memory directory from template: {}",
            target.display()
        ),
        Err(e) => tracing::warn!("Failed to sync memory directory: {}", e),
    }
}

/// Recursively sync `src` (template) into `dst` (runtime memory dir).
fn sync_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            sync_dir(&src_path, &dst_path)?;
        } else {
            maybe_copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Copy `src` to `dst` when:
///   - dst doesn't exist (first run), OR
///   - dst content is identical to src (template hasn't changed, refresh it)
///
/// Otherwise (dst content differs from src) skip — MemoryAgent has written
/// user data that should be preserved.
fn maybe_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.exists() {
        let src_bytes = fs::read(src)?;
        let dst_bytes = fs::read(dst)?;
        if src_bytes != dst_bytes {
            // Content differs — user data written by MemoryAgent, preserve it.
            return Ok(());
        }
        // Content identical — template is up-to-date or was reverted, refresh.
    }
    fs::copy(src, dst)?;
    Ok(())
}

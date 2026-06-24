//! Sync runtime bundle directory from the built-in OKF template.

use std::fs;
use std::path::Path;

/// Built-in default bundle template (SKILL.md, AGENT.md, references/, assets/, index.md, log.md).
pub const TEMPLATE_DIR: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/src/super_brain/templates/default");

/// Seed `target` from the built-in template. Idempotent — user-written files are preserved.
pub fn seed_super_brain_dir(target: &Path) {
    let template = Path::new(TEMPLATE_DIR);
    if !template.exists() {
        tracing::warn!("Bundle template not found: {}", template.display());
        return;
    }
    match sync_dir(template, target) {
        Ok(()) => tracing::info!(
            "Synchronized super-brain from template: {}",
            target.display()
        ),
        Err(e) => tracing::warn!("Failed to sync super-brain: {}", e),
    }
}

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

fn maybe_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    if dst.exists() {
        let src_bytes = fs::read(src)?;
        let dst_bytes = fs::read(dst)?;
        if src_bytes != dst_bytes {
            return Ok(());
        }
    }
    fs::copy(src, dst)?;
    Ok(())
}

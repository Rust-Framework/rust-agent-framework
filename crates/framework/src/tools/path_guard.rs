//! Shared path resolution with traversal protection.
//!
//! All filesystem tools should use `resolve_safe()` instead of
//! raw path joining, to prevent `../` and absolute-path escapes.

use rust_agent_core::AgentError;
use std::path::{Path, PathBuf};

/// Resolve a user-supplied path against a base directory with traversal protection.
///
/// Returns the canonical path, or an error if:
/// - The path does not exist or cannot be canonicalized
/// - The canonical path escapes `base_dir`
pub fn resolve_safe(base_dir: &Path, path: &str) -> Result<PathBuf, AgentError> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else if path.is_empty() || path == "." {
        base_dir.to_path_buf()
    } else {
        base_dir.join(path)
    };

    let canonical = candidate.canonicalize().map_err(|e| {
        AgentError::ToolError(format!("Path does not exist or cannot be resolved: {}", e))
    })?;

    let canonical_base = base_dir
        .canonicalize()
        .unwrap_or_else(|_| base_dir.to_path_buf());
    if !canonical.starts_with(&canonical_base) {
        return Err(AgentError::ToolError("Path traversal denied".into()));
    }
    Ok(canonical)
}

/// Resolve a user-supplied path with traversal protection, but allow
/// paths that don't exist yet (for write/create operations).
pub fn resolve_safe_new(base_dir: &Path, path: &str) -> Result<PathBuf, AgentError> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else if path.is_empty() || path == "." {
        base_dir.to_path_buf()
    } else {
        base_dir.join(path)
    };

    // Check parent dir for ".." escape before canonicalization
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    // Already at root — trying to escape
                    return Err(AgentError::ToolError("Path traversal denied".into()));
                }
            }
            other => normalized.push(other),
        }
    }

    let canonical_base = base_dir
        .canonicalize()
        .unwrap_or_else(|_| base_dir.to_path_buf());
    // Try canonicalize — if path doesn't exist yet, manually check prefix
    if let Ok(canon) = normalized.canonicalize() {
        if !canon.starts_with(&canonical_base) {
            return Err(AgentError::ToolError("Path traversal denied".into()));
        }
    } else {
        // Path doesn't exist yet — walk up the ancestor chain until we find
        // one that exists, then verify it's within base_dir.
        let mut current = normalized.as_path();
        loop {
            match current.canonicalize() {
                Ok(canon) => {
                    if !canon.starts_with(&canonical_base) {
                        return Err(AgentError::ToolError("Path traversal denied".into()));
                    }
                    break;
                }
                Err(_) => match current.parent() {
                    Some(parent) => current = parent,
                    None => {
                        return Err(AgentError::ToolError(
                            "Path resolution failed: no existing ancestor found".into(),
                        ));
                    }
                },
            }
        }
    }
    Ok(normalized)
}

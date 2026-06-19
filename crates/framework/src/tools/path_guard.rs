//! 共享的路径解析与目录穿越防护工具。

use rust_agent_core::AgentError;
use std::path::{Path, PathBuf};

/// 工具内部用 scope 检测结果
#[derive(Debug, Clone)]
pub(crate) enum ScopeStatus {
    InScope,
    OutsideScope,
    NotApplicable,
}

impl ScopeStatus {
    pub fn to_label(&self) -> &str {
        match self {
            ScopeStatus::InScope => "workspace",
            ScopeStatus::OutsideScope => "outside_workspace",
            ScopeStatus::NotApplicable => "none",
        }
    }
}

// ── 内部辅助 ──────────────────────────────────────────────────────────

fn candidate(base_dir: &Path, path: &str) -> PathBuf {
    if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else if path.is_empty() || path == "." {
        base_dir.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn scope_status(resolved: &Path, scope_root: Option<&Path>) -> ScopeStatus {
    match scope_root {
        Some(r) => {
            let cr = r.canonicalize().unwrap_or_else(|_| r.to_path_buf());
            if resolved.starts_with(&cr) { ScopeStatus::InScope } else { ScopeStatus::OutsideScope }
        }
        None => ScopeStatus::NotApplicable,
    }
}

/// 手动消解 `..` 组件并检测逃逸（用于路径尚不存在的写操作）。
fn normalize(candidate: &Path) -> Result<PathBuf, AgentError> {
    let mut out = PathBuf::new();
    for c in candidate.components() {
        match c {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    return Err(AgentError::ToolError("Path traversal denied".into()));
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

// ── 公开 API ──────────────────────────────────────────────────────────

/// 解析并校验**已存在**路径（读操作）。错误情况：
/// - 路径不存在或无法规范化
/// - 规范路径逃逸出 `base_dir`
pub(crate) fn resolve_safe(
    base_dir: &Path,
    path: &str,
    scope_root: Option<&Path>,
) -> Result<(PathBuf, ScopeStatus), AgentError> {
    let c = candidate(base_dir, path);
    let canonical = c.canonicalize().map_err(|e| {
        AgentError::ToolError(format!("Path does not exist or cannot be resolved: {}", e))
    })?;
    let canon_base = base_dir.canonicalize().unwrap_or_else(|_| base_dir.to_path_buf());
    if !canonical.starts_with(&canon_base) {
        return Err(AgentError::ToolError("Path traversal denied".into()));
    }
    let s = scope_status(&canonical, scope_root);
    Ok((canonical, s))
}

/// 解析并校验**可能不存在**的路径（写/创建操作）。
pub(crate) fn resolve_safe_new(
    base_dir: &Path,
    path: &str,
    scope_root: Option<&Path>,
) -> Result<(PathBuf, ScopeStatus), AgentError> {
    let c = candidate(base_dir, path);
    let normalized = normalize(&c)?;
    let canon_base = base_dir.canonicalize().unwrap_or_else(|_| base_dir.to_path_buf());

    if let Ok(canon) = normalized.canonicalize() {
        if !canon.starts_with(&canon_base) {
            return Err(AgentError::ToolError("Path traversal denied".into()));
        }
        let s = scope_status(&canon, scope_root);
        return Ok((canon, s));
    }

    let mut cur = normalized.as_path();
    loop {
        match cur.canonicalize() {
            Ok(canon) => {
                if !canon.starts_with(&canon_base) {
                    return Err(AgentError::ToolError("Path traversal denied".into()));
                }
                let s = scope_status(&normalized, scope_root);
                return Ok((normalized, s));
            }
            Err(_) => match cur.parent() {
                Some(p) => cur = p,
                None => return Err(AgentError::ToolError(
                    "Path resolution failed: no existing ancestor found".into(),
                )),
            },
        }
    }
}

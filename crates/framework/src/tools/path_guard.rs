//! 共享的路径解析与目录穿越防护工具。
//!
//! 所有文件系统工具应使用 `resolve_safe()` 而非原始路径拼接，
//! 以防止 `../` 和绝对路径逃逸。

use rust_agent_core::AgentError;
use std::path::{Path, PathBuf};

/// 工具内部用 scope 检测结果
#[derive(Debug, Clone)]
pub(crate) enum ScopeStatus {
    /// 路径在工作区范围内
    InScope,
    /// 路径在工作区范围外
    OutsideScope,
    /// 无 scope 设置，无需判断
    NotApplicable,
}

impl ScopeStatus {
    /// 转为 JSON 响应中的 `"scope"` 标签值
    pub fn to_label(&self) -> &str {
        match self {
            ScopeStatus::InScope => "workspace",
            ScopeStatus::OutsideScope => "outside_workspace",
            ScopeStatus::NotApplicable => "none",
        }
    }
}

/// 将用户提供的路径相对于基础目录解析，含目录穿越防护。
///
/// `scope_root`: 可选的工作区根路径，用于判断操作是否在范围内。
/// 当 `scope_root` 为 `None` 时，`ScopeStatus` 返回 `NotApplicable`。
///
/// 返回 (规范化路径, scope状态)，以下情况返回错误：
/// - 路径不存在或无法规范化
/// - 规范路径逃逸出 `base_dir`
pub(crate) fn resolve_safe(
    base_dir: &Path,
    path: &str,
    scope_root: Option<&Path>,
) -> Result<(PathBuf, ScopeStatus), AgentError> {
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

    // 遍历安全检测（独立于 scope 逻辑）
    if !canonical.starts_with(&canonical_base) {
        return Err(AgentError::ToolError("Path traversal denied".into()));
    }

    // Scope 检测
    let scope_status = match scope_root {
        Some(root) => {
            let canonical_root = root
                .canonicalize()
                .unwrap_or_else(|_| root.to_path_buf());
            if canonical.starts_with(&canonical_root) {
                ScopeStatus::InScope
            } else {
                ScopeStatus::OutsideScope
            }
        }
        None => ScopeStatus::NotApplicable,
    };

    Ok((canonical, scope_status))
}

/// 将用户提供的路径相对于基础目录解析，含目录穿越防护，允许路径尚不存在（用于写/创建操作）。
///
/// `scope_root` 的语义同 `resolve_safe`。
pub(crate) fn resolve_safe_new(
    base_dir: &Path,
    path: &str,
    scope_root: Option<&Path>,
) -> Result<(PathBuf, ScopeStatus), AgentError> {
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

    // Scope root 规范化（用于后续 scope 检测）
    let canonical_scope_root = scope_root
        .map(|r| r.canonicalize().unwrap_or_else(|_| r.to_path_buf()));

    // Try canonicalize — if path doesn't exist yet, manually check prefix
    if let Ok(canon) = normalized.canonicalize() {
        if !canon.starts_with(&canonical_base) {
            return Err(AgentError::ToolError("Path traversal denied".into()));
        }
        let scope_status = match &canonical_scope_root {
            Some(root) if canon.starts_with(root) => ScopeStatus::InScope,
            Some(_) => ScopeStatus::OutsideScope,
            None => ScopeStatus::NotApplicable,
        };
        return Ok((canon, scope_status));
    }

    // Path doesn't exist yet — walk up the ancestor chain until we find
    // one that exists, then verify it's within base_dir.
    let mut current = normalized.as_path();
    loop {
        match current.canonicalize() {
            Ok(canon) => {
                if !canon.starts_with(&canonical_base) {
                    return Err(AgentError::ToolError("Path traversal denied".into()));
                }
                let scope_status = match &canonical_scope_root {
                    Some(root) if normalized.starts_with(root) => ScopeStatus::InScope,
                    Some(_) => ScopeStatus::OutsideScope,
                    None => ScopeStatus::NotApplicable,
                };
                return Ok((normalized, scope_status));
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

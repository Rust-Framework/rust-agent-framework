//! 代码沙箱抽象 — 核心仅定义契约，具体实现位于 `rust-agent-sandbox` 等扩展 crate。
//!
//! 对标 MAF CodeInterpreter：核心提供 `ICodeSandbox` + 请求/结果类型，
//! 运行时通过 `ITool` 或工厂注册注入，避免 wasmtime / 容器引擎污染 core。

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{AgentError, Result};

/// 沙箱支持的语言/运行时标识（开放字符串，插件可扩展）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxLanguage(pub String);

impl SandboxLanguage {
    pub fn python() -> Self {
        Self("python".into())
    }

    pub fn javascript() -> Self {
        Self("javascript".into())
    }

    pub fn shell() -> Self {
        Self("shell".into())
    }

    pub fn wat() -> Self {
        Self("wat".into())
    }

    pub fn wasm() -> Self {
        Self("wasm".into())
    }
}

/// 沙箱执行请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxRequest {
    /// 目标语言/运行时。
    pub language: SandboxLanguage,
    /// 待执行源码或脚本片段。
    pub code: String,
    /// 单次执行超时；未设置则由实现方默认。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<Duration>,
    /// 可选工作目录根（配合 `WorkspaceScope` 限制可见路径）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<PathBuf>,
    /// 附加 stdin 或上下文参数（JSON）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

/// 沙箱执行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    #[serde(default)]
    pub artifacts: Vec<SandboxArtifact>,
}

/// 沙箱产出的文件或结构化输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxArtifact {
    pub name: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

impl SandboxResult {
    pub fn success(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            artifacts: Vec::new(),
        }
    }

    pub fn failed(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
            artifacts: Vec::new(),
        }
    }

    pub fn ok(&self) -> bool {
        self.exit_code == 0
    }
}

/// 代码沙箱执行器 — 由 `rust-agent-sandbox` 等扩展 crate 实现。
#[async_trait]
pub trait ICodeSandbox: Send + Sync {
    /// 在隔离环境中执行代码并返回 stdout/stderr/exit code。
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxResult>;

    /// 人类可读的后端名称（日志/诊断）。
    fn backend_name(&self) -> &str {
        "sandbox"
    }
}

/// 将沙箱错误映射为 Agent 错误。
pub fn sandbox_error(msg: impl Into<String>) -> AgentError {
    AgentError::ToolError(msg.into())
}

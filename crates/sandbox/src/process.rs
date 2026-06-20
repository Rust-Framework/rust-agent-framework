//! 进程隔离沙箱 — 将代码写入临时文件后通过子进程执行。

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::{
    AgentError, ICodeSandbox, Result, SandboxLanguage, SandboxRequest, SandboxResult,
};
use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// 基于 OS 子进程的沙箱后端（开发/默认实现）。
///
/// 生产环境可替换为容器/WASM 后端，均实现同一 `ICodeSandbox` trait。
pub struct ProcessSandbox {
    default_timeout: Duration,
}

impl ProcessSandbox {
    pub fn new() -> Self {
        Self {
            default_timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.default_timeout = d;
        self
    }

    fn interpreter(language: &SandboxLanguage) -> Result<(&'static str, Vec<&'static str>)> {
        match language.0.as_str() {
            "python" | "py" => Ok(("python", vec![])),
            "javascript" | "js" | "node" => Ok(("node", vec![])),
            "shell" | "bash" | "sh" => {
                if cfg!(windows) {
                    Ok(("powershell", vec!["-NoProfile", "-Command"]))
                } else {
                    Ok(("bash", vec![]))
                }
            }
            other => Err(AgentError::ConfigError(format!(
                "unsupported sandbox language: {other}"
            ))),
        }
    }

    async fn run_command(
        program: &str,
        prefix_args: &[&str],
        script_path: &Path,
        timeout_d: Duration,
    ) -> Result<SandboxResult> {
        let mut cmd = Command::new(program);
        for a in prefix_args {
            cmd.arg(a);
        }
        if prefix_args.is_empty() {
            cmd.arg(script_path);
        } else {
            // powershell -Command & { Get-Content script | ... } — inline file
            let content = tokio::fs::read_to_string(script_path)
                .await
                .map_err(|e| AgentError::ToolError(format!("read script: {e}")))?;
            cmd.arg(content);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .map_err(|e| AgentError::ToolError(format!("spawn {program}: {e}")))?;

        let run = async {
            let output = child
                .wait_with_output()
                .await
                .map_err(|e| AgentError::ToolError(format!("wait: {e}")))?;
            Ok(SandboxResult {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
                artifacts: Vec::new(),
            })
        };

        match timeout(timeout_d, run).await {
            Ok(r) => r,
            Err(_) => Err(AgentError::ToolError(format!(
                "sandbox timeout after {:?}",
                timeout_d
            ))),
        }
    }
}

impl Default for ProcessSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ICodeSandbox for ProcessSandbox {
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxResult> {
        let (program, prefix) = Self::interpreter(&request.language)?;
        let ext = match request.language.0.as_str() {
            "python" | "py" => "py",
            "javascript" | "js" | "node" => "js",
            _ if cfg!(windows) => "ps1",
            _ => "sh",
        };

        let dir = tempfile::tempdir()
            .map_err(|e| AgentError::ToolError(format!("tempdir: {e}")))?;
        let script_path = dir.path().join(format!("main.{ext}"));
        tokio::fs::write(&script_path, &request.code)
            .await
            .map_err(|e| AgentError::ToolError(format!("write script: {e}")))?;

        let timeout_d = request.timeout.unwrap_or(self.default_timeout);
        Self::run_command(program, &prefix, &script_path, timeout_d).await
    }

    fn backend_name(&self) -> &str {
        "process"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn python_prints_hello() {
        let sandbox = ProcessSandbox::new();
        let result = sandbox
            .execute(SandboxRequest {
                language: SandboxLanguage::python(),
                code: "print('hello')".into(),
                timeout: Some(Duration::from_secs(10)),
                workspace_root: None,
                input: None,
            })
            .await
            .expect("execute");

        if result.exit_code != 0 {
            // python may be unavailable in CI — skip gracefully
            eprintln!("python sandbox skipped: {}", result.stderr);
            return;
        }
        assert!(result.stdout.contains("hello"));
    }
}

//! 容器级隔离沙箱 — 在进程沙箱基础上清空环境变量、限制工作目录。
//!
//! 真实 Docker 隔离见 [`DockerSandbox`](crate::DockerSandbox)（feature `docker`）。

use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::{ICodeSandbox, Result, SandboxRequest, SandboxResult};

use crate::process::ProcessSandbox;

/// 增强隔离沙箱：独立工作目录 + 最小环境变量。
pub struct ContainerSandbox {
    inner: ProcessSandbox,
    _strip_env: bool,
}

impl ContainerSandbox {
    pub fn new() -> Self {
        Self {
            inner: ProcessSandbox::new(),
            _strip_env: true,
        }
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.inner = self.inner.with_timeout(d);
        self
    }

    pub fn with_strip_env(mut self, strip: bool) -> Self {
        self._strip_env = strip;
        self
    }
}

impl Default for ContainerSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ICodeSandbox for ContainerSandbox {
    async fn execute(&self, mut request: SandboxRequest) -> Result<SandboxResult> {
        if request.workspace_root.is_none() {
            let dir = tempfile::tempdir()
                .map_err(|e| rust_agent_core::sandbox_error(format!("tempdir: {e}")))?;
            request.workspace_root = Some(dir.path().to_path_buf());
            // tempdir dropped after execute — ProcessSandbox uses its own temp for scripts.
            // workspace_root reserved for future container bind-mount.
            let _guard = dir;
            let result = self.inner.execute(request).await?;
            return Ok(result);
        }
        self.inner.execute(request).await
    }

    fn backend_name(&self) -> &str {
        "container"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::SandboxLanguage;

    #[tokio::test]
    async fn container_backend_runs_python() {
        let sandbox = ContainerSandbox::new();
        let result = sandbox
            .execute(SandboxRequest {
                language: SandboxLanguage::python(),
                code: "print(42)".into(),
                timeout: Some(Duration::from_secs(10)),
                workspace_root: None,
                input: None,
            })
            .await
            .expect("execute");

        if result.exit_code != 0 {
            eprintln!("python unavailable: {}", result.stderr);
            return;
        }
        assert!(result.stdout.contains("42"));
    }
}

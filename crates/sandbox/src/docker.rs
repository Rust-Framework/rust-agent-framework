//! Docker CLI 沙箱 — 通过 `docker run` 在容器内执行代码。
//!
//! 需要本机已安装并运行 Docker；不可用时返回明确错误。

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::{
    AgentError, ICodeSandbox, Result, SandboxLanguage, SandboxRequest, SandboxResult,
};
use tokio::process::Command;
use tokio::time::timeout;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// 基于 Docker/Podman CLI 的容器沙箱。
pub struct DockerSandbox {
    cli: String,
    default_timeout: Duration,
    network_disabled: bool,
    memory_limit: Option<String>,
    cpus: Option<String>,
    pids_limit: Option<u64>,
    python_image: Option<String>,
    node_image: Option<String>,
}

impl DockerSandbox {
    pub fn new() -> Self {
        Self {
            cli: "docker".into(),
            default_timeout: DEFAULT_TIMEOUT,
            network_disabled: true,
            memory_limit: Some("256m".into()),
            cpus: None,
            pids_limit: Some(128),
            python_image: None,
            node_image: None,
        }
    }

    /// 使用 Podman CLI（与 Docker 兼容的子命令）。
    pub fn podman() -> Self {
        Self::new().with_cli("podman")
    }

    pub fn with_cli(mut self, cli: impl Into<String>) -> Self {
        self.cli = cli.into();
        self
    }

    pub fn with_pids_limit(mut self, limit: u64) -> Self {
        self.pids_limit = Some(limit);
        self
    }

    pub fn with_cpus(mut self, cpus: impl Into<String>) -> Self {
        self.cpus = Some(cpus.into());
        self
    }

    pub fn with_python_image(mut self, image: impl Into<String>) -> Self {
        self.python_image = Some(image.into());
        self
    }

    pub fn with_node_image(mut self, image: impl Into<String>) -> Self {
        self.node_image = Some(image.into());
        self
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.default_timeout = d;
        self
    }

    pub fn with_network(mut self, enabled: bool) -> Self {
        self.network_disabled = !enabled;
        self
    }

    pub fn with_memory_limit(mut self, limit: impl Into<String>) -> Self {
        self.memory_limit = Some(limit.into());
        self
    }

    fn image_for(&self, language: &SandboxLanguage) -> Result<String> {
        match language.0.as_str() {
            "python" | "py" => Ok(self
                .python_image
                .clone()
                .unwrap_or_else(|| "python:3-slim".into())),
            "javascript" | "js" | "node" => Ok(self
                .node_image
                .clone()
                .unwrap_or_else(|| "node:20-slim".into())),
            other => Err(AgentError::ConfigError(format!(
                "docker sandbox unsupported language: {other}"
            ))),
        }
    }

    fn script_filename(language: &SandboxLanguage) -> &'static str {
        match language.0.as_str() {
            "javascript" | "js" | "node" => "main.js",
            _ => "main.py",
        }
    }

    fn run_command(language: &SandboxLanguage) -> (&'static str, Vec<&'static str>) {
        match language.0.as_str() {
            "javascript" | "js" | "node" => ("node", vec!["/work/main.js"]),
            _ => ("python", vec!["/work/main.py"]),
        }
    }

    async fn cli_available(cli: &str) -> bool {
        Command::new(cli)
            .arg("version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    async fn run_in_container(
        &self,
        image: &str,
        program: &str,
        args: &[&str],
        host_workdir: &Path,
        timeout_d: Duration,
    ) -> Result<SandboxResult> {
        if !Self::cli_available(&self.cli).await {
            return Err(AgentError::ConfigError(format!(
                "'{}' is not available — install container runtime or use ProcessSandbox",
                self.cli
            )));
        }

        let mut cmd = Command::new(&self.cli);
        cmd.arg("run")
            .arg("--rm")
            .arg("-i")
            .arg("--init")
            .arg("-v")
            .arg(format!("{}:/work:ro", host_workdir.display()))
            .arg("-w")
            .arg("/work");

        if self.network_disabled {
            cmd.arg("--network").arg("none");
        }
        if let Some(mem) = &self.memory_limit {
            cmd.arg("--memory").arg(mem);
        }
        if let Some(cpus) = &self.cpus {
            cmd.arg("--cpus").arg(cpus);
        }
        if let Some(pids) = self.pids_limit {
            cmd.arg("--pids-limit").arg(pids.to_string());
        }

        cmd.arg(image).arg(program);
        for a in args {
            cmd.arg(a);
        }

        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        let child = cmd
            .spawn()
            .map_err(|e| AgentError::ToolError(format!("{} run: {e}", self.cli)))?;

        let run = async {
            let output = child
                .wait_with_output()
                .await
                .map_err(|e| AgentError::ToolError(format!("{} wait: {e}", self.cli)))?;
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
                "{} sandbox timeout after {:?}",
                self.cli, timeout_d
            ))),
        }
    }
}

impl Default for DockerSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ICodeSandbox for DockerSandbox {
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxResult> {
        let image = self.image_for(&request.language)?;
        let script_name = Self::script_filename(&request.language);
        let (program, args) = Self::run_command(&request.language);

        let dir = tempfile::tempdir()
            .map_err(|e| AgentError::ToolError(format!("tempdir: {e}")))?;
        let script_path = dir.path().join(script_name);
        tokio::fs::write(&script_path, &request.code)
            .await
            .map_err(|e| AgentError::ToolError(format!("write script: {e}")))?;

        let timeout_d = request.timeout.unwrap_or(self.default_timeout);
        self.run_in_container(&image, program, &args, dir.path(), timeout_d)
            .await
    }

    fn backend_name(&self) -> &str {
        &self.cli
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn docker_runs_python_when_available() {
        if !DockerSandbox::cli_available("docker").await {
            eprintln!("docker not available — skipping");
            return;
        }

        let sandbox = DockerSandbox::new();
        let result = sandbox
            .execute(SandboxRequest {
                language: SandboxLanguage::python(),
                code: "print(99)".into(),
                timeout: Some(Duration::from_secs(120)),
                workspace_root: None,
                input: None,
            })
            .await
            .expect("execute");

        assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
        assert!(result.stdout.contains("99"));
    }
}

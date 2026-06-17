use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::Result;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// 技能脚本执行器 trait。
///
/// 对标 MAF SubprocessScriptRunner。用户可实现自定义 Runner（沙箱、容器等）。
#[async_trait]
pub trait AgentSkillScriptRunner: Send + Sync {
    /// 执行脚本，返回 stdout。
    /// `timeout_secs` overrides the runner's configured default per-call when provided.
    async fn run(
        &self,
        skill_name: &str,
        script_path: &Path,
        args: Option<Vec<String>>,
        timeout_secs: Option<u64>,
    ) -> Result<String>;
}

/// 默认子进程执行器。
pub struct SubprocessScriptRunner {
    timeout_secs: Option<u64>,
}

impl SubprocessScriptRunner {
    pub fn new() -> Self {
        Self { timeout_secs: None }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }
}

impl Default for SubprocessScriptRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentSkillScriptRunner for SubprocessScriptRunner {
    async fn run(
        &self,
        _skill_name: &str,
        script_path: &Path,
        args: Option<Vec<String>>,
        timeout_secs: Option<u64>,
    ) -> Result<String> {
        // 根据扩展名选择解释器
        let ext = script_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let (program, mut cmd_parts): (&str, Vec<String>) = match ext {
            "py" => ("python", vec![script_path.to_string_lossy().to_string()]),
            "js" => ("node", vec![script_path.to_string_lossy().to_string()]),
            "sh" if cfg!(windows) => {
                ("bash", vec![script_path.to_string_lossy().to_string()])
            }
            "ps1" => (
                "powershell",
                vec![
                    "-File".to_string(),
                    script_path.to_string_lossy().to_string(),
                ],
            ),
            _ => {
                if cfg!(windows) {
                    (
                        "cmd",
                        vec![
                            "/c".to_string(),
                            script_path.to_string_lossy().to_string(),
                        ],
                    )
                } else {
                    (
                        "sh",
                        vec![
                            "-c".to_string(),
                            script_path.to_string_lossy().to_string(),
                        ],
                    )
                }
            }
        };

        if let Some(a) = &args {
            cmd_parts.extend(a.iter().cloned());
        }

        let timeout_dur = Duration::from_secs(
            timeout_secs
                .or(self.timeout_secs)
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
        );

        let output = tokio::time::timeout(
            timeout_dur,
            tokio::task::spawn_blocking(move || {
                std::process::Command::new(program)
                    .args(&cmd_parts)
                    .output()
            }),
        )
        .await
        .map_err(|_| {
            rust_agent_core::AgentError::ToolError("Script execution timed out".into())
        })?
        .map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Failed to execute script (join error): {}",
                e
            ))
        })?
        .map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Failed to execute script: {}",
                e
            ))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            Err(rust_agent_core::AgentError::ToolError(format!(
                "Script exited with code {:?}\nstderr: {}",
                output.status.code(),
                stderr
            )))
        }
    }
}

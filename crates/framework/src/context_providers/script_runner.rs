use std::path::Path;

use async_trait::async_trait;
use rust_agent_core::Result;

/// 技能脚本执行器 trait。
///
/// 对标 MAF SubprocessScriptRunner。用户可实现自定义 Runner（沙箱、容器等）。
#[async_trait]
pub trait AgentSkillScriptRunner: Send + Sync {
    /// 执行脚本，返回 stdout。
    async fn run(
        &self,
        skill_name: &str,
        script_path: &Path,
        args: Option<Vec<String>>,
    ) -> Result<String>;
}

/// 默认子进程执行器。
pub struct SubprocessScriptRunner;

#[async_trait]
impl AgentSkillScriptRunner for SubprocessScriptRunner {
    async fn run(
        &self,
        _skill_name: &str,
        script_path: &Path,
        args: Option<Vec<String>>,
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

        let output = std::process::Command::new(program)
            .args(&cmd_parts)
            .output()
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

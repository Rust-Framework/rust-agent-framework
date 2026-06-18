use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};
use rust_agent_macros::tool;

use super::path_guard::{resolve_safe, ScopeStatus};

const MAX_OUTPUT_BYTES: usize = 100 * 1024;
const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[tool(description = "Executes a shell command and returns the output (stdout + stderr) and exit code.")]
pub struct RunCommand {
    pub scope: Option<Arc<WorkspaceScope>>,
    pub timeout_secs: Option<u64>,
}

impl IScopeTool for RunCommand {
    fn create_scoped(&self, scope: Arc<WorkspaceScope>) -> Arc<dyn ITool> {
        Arc::new(RunCommand {
            scope: Some(scope),
            timeout_secs: self.timeout_secs,
        })
    }
}

fn resolve_working_dir(base_dir: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else if path.is_empty() || path == "." {
        base_dir.to_path_buf()
    } else {
        base_dir.join(p)
    }
}

fn truncate_bytes(data: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(if data.len() <= max { data } else { &data[..max] }).to_string();
    if data.len() > max {
        format!("{}...[truncated]", s)
    } else {
        s
    }
}

impl RunCommand {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            command: String,
            working_dir: Option<String>,
            timeout_secs: Option<u64>,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!("Argument deserialization failed: {}", e))
        })?;

        let (program, shell_args) = if cfg!(windows) {
            ("cmd", vec!["/c".to_string(), args.command.clone()])
        } else {
            ("sh", vec!["-c".to_string(), args.command.clone()])
        };

        let mut cmd = Command::new(program);
        cmd.args(&shell_args);
        cmd.stdin(std::process::Stdio::null());

        // Resolve working directory
        let base_dir = self
            .scope
            .as_ref()
            .map(|s| s.root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let cwd = match args.working_dir {
            Some(ref dir) => resolve_working_dir(&base_dir, dir),
            None => base_dir.clone(),
        };
        cmd.current_dir(&cwd);

        // Scope detection for working_dir
        let scope_label = match self.scope.as_ref() {
            Some(scope) => {
                let scope_root = scope.root.as_path();
                // Check if cwd is within scope
                match resolve_safe(&base_dir, cwd.to_string_lossy().as_ref(), Some(scope_root)) {
                    Ok((_, status)) => {
                        if scope.policy == ScopePolicy::DenyOutside
                            && matches!(status, ScopeStatus::OutsideScope)
                        {
                            return Ok(ToolResult::error(
                                "Access denied: working directory is outside workspace boundary",
                            ));
                        }
                        status.to_label().to_string()
                    }
                    Err(_) => "none".to_string(),
                }
            }
            None => "none".to_string(),
        };

        let timeout_dur = Duration::from_secs(
            args.timeout_secs
                .or(self.timeout_secs)
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
        );

        match tokio::time::timeout(timeout_dur, tokio::task::spawn_blocking(move || cmd.output()))
            .await
        {
            Err(_elapsed) => Ok(ToolResult::error("Command execution timed out")),
            Ok(Err(join_err)) => Ok(ToolResult::error(format!(
                "Command execution failed: {}",
                join_err
            ))),
            Ok(Ok(Err(io_err))) => Ok(ToolResult::error(format!(
                "Failed to execute command: {}",
                io_err
            ))),
            Ok(Ok(Ok(output))) => {
                let stdout = truncate_bytes(&output.stdout, MAX_OUTPUT_BYTES);
                let stderr = truncate_bytes(&output.stderr, MAX_OUTPUT_BYTES);
                let exit_code = output.status.code().unwrap_or(-1);

                Ok(ToolResult::success(serde_json::json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": exit_code,
                    "scope": scope_label,
                })))
            }
        }
    }
}

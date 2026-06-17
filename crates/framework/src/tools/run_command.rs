use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use super::{err_response, ok_response};

const MAX_OUTPUT_BYTES: usize = 100 * 1024; // 100 KB
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Executes a shell command and returns the output (stdout + stderr) and exit code.
///
/// The working directory is resolved against `base_dir`:
/// - If `working_dir` is an absolute path → pass through unchanged.
/// - If `working_dir` is a relative path → join with `base_dir`.
/// - If `working_dir` is None → use `base_dir` directly (not process CWD).
pub struct RunCommand {
    base_dir: PathBuf,
    timeout_secs: Option<u64>,
}

impl RunCommand {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            timeout_secs: None,
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() { p.to_path_buf() }
        else if path.is_empty() || path == "." { self.base_dir.clone() }
        else { self.base_dir.join(p) }
    }

    async fn call(
        &self,
        command: String,
        working_dir: Option<String>,
        timeout_secs: Option<u64>,
    ) -> String {
        let (program, args) = if cfg!(windows) {
            ("cmd", vec!["/c".to_string(), command.clone()])
        } else {
            ("sh", vec!["-c".to_string(), command.clone()])
        };

        let mut cmd = Command::new(program);
        cmd.args(&args);
        cmd.stdin(std::process::Stdio::null());

        // Resolve working directory against base_dir
        let cwd = match working_dir {
            Some(dir) => self.resolve(&dir),
            None => self.base_dir.clone(),
        };
        cmd.current_dir(&cwd);

        let timeout_dur = Duration::from_secs(
            timeout_secs
                .or(self.timeout_secs)
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
        );

        match tokio::time::timeout(
            timeout_dur,
            tokio::task::spawn_blocking(move || cmd.output()),
        )
        .await
        {
            Err(_elapsed) => err_response("Command execution timed out"),
            Ok(Err(join_err)) => {
                err_response(&format!("Command execution failed: {}", join_err))
            }
            Ok(Ok(Err(io_err))) => {
                err_response(&format!("Failed to execute command: {}", io_err))
            }
            Ok(Ok(Ok(output))) => {
                let stdout = truncate_bytes(&output.stdout, MAX_OUTPUT_BYTES);
                let stderr = truncate_bytes(&output.stderr, MAX_OUTPUT_BYTES);
                let exit_code = output.status.code().unwrap_or(-1);

                ok_response(serde_json::json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": exit_code,
                }))
            }
        }
    }
}

fn truncate_bytes(data: &[u8], max: usize) -> String {
    let s = String::from_utf8_lossy(
        if data.len() <= max { data } else { &data[..max] }
    ).to_string();
    if data.len() > max {
        format!("{}...[truncated]", s)
    } else {
        s
    }
}

impl Default for RunCommand {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

#[async_trait]
impl ITool for RunCommand {
    fn name(&self) -> &str {
        "run_command"
    }

    fn description(&self) -> &str {
        "Executes a shell command and returns the output (stdout + stderr) and exit code."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory for the command (optional; absolute, or relative to the agent's working directory; defaults to agent's working directory)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds for the command (optional; defaults to 30). Increase for long-running commands like builds."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct Args {
            command: String,
            working_dir: Option<String>,
            timeout_secs: Option<u64>,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;
        Ok(self
            .call(args.command, args.working_dir, args.timeout_secs)
            .await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_echo() {
        let cmd = if cfg!(windows) { "echo hello" } else { "echo hello" };
        let result = RunCommand::default()
            .execute(serde_json::json!({"command": cmd}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["exit_code"], 0);
        assert!(v["data"]["stdout"].as_str().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_run_nonexistent_command() {
        let result = RunCommand::default()
            .execute(serde_json::json!({"command": "nonexistent_command_xyz"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert!(!v["ok"].as_bool().unwrap() || v["data"]["exit_code"].as_i64().unwrap() != 0);
    }

    #[tokio::test]
    async fn test_run_command_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();

        let tool = RunCommand::new(dir.path());
        let cmd = if cfg!(windows) {
            "cd".to_string()
        } else {
            "pwd".to_string()
        };
        let result = tool
            .execute(serde_json::json!({"command": cmd}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["exit_code"], 0);
    }
}

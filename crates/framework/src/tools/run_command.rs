use std::process::Command;

use rust_agent_macros::tool;

use super::{err_response, ok_response};

const MAX_OUTPUT_BYTES: usize = 100 * 1024; // 100 KB

// Avoid using `reqwest::Client` here — we need a blocking call.
// The #[tool] macro generates an async execute() which wraps our async fn.

#[tool(description = "Executes a shell command and returns the output (stdout + stderr) and exit code.")]
async fn run_command(
    #[param(desc = "Shell command to execute")] command: String,
    #[param(desc = "Working directory for the command (optional, defaults to current)")] working_dir: Option<String>,
) -> String {
    // Build command
    let (program, args) = if cfg!(windows) {
        ("cmd", vec!["/c".to_string(), command.clone()])
    } else {
        ("sh", vec!["-c".to_string(), command.clone()])
    };

    let mut cmd = Command::new(program);
    cmd.args(&args);
    cmd.stdin(std::process::Stdio::null());

    if let Some(dir) = &working_dir {
        cmd.current_dir(dir);
    }

    match cmd.output() {
        Ok(output) => {
            let stdout = truncate_bytes(&output.stdout, MAX_OUTPUT_BYTES);
            let stderr = truncate_bytes(&output.stderr, MAX_OUTPUT_BYTES);
            let exit_code = output.status.code().unwrap_or(-1);

            ok_response(serde_json::json!({
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": exit_code,
            }))
        }
        Err(e) => err_response(&format!("Failed to execute command: {}", e)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ITool;

    #[tokio::test]
    async fn test_run_echo() {
        let cmd = if cfg!(windows) { "echo hello" } else { "echo hello" };
        let result = RunCommand
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
        let result = RunCommand
            .execute(serde_json::json!({"command": "nonexistent_command_xyz"}))
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        // Command should run but return non-zero exit code
        // (or fail entirely — both are acceptable)
        assert!(!v["ok"].as_bool().unwrap() || v["data"]["exit_code"].as_i64().unwrap() != 0);
    }
}

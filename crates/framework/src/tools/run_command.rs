use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rust_agent_core::{IScopeTool, ITool, ScopePolicy, ToolResult, WorkspaceScope};

use super::path_guard::{resolve_safe, ScopeStatus};

// 默认截断阈值（各 output_level 的基础值）
const MAX_STDOUT_INFO: usize = 500 * 1024; // info 模式 stdout
const MAX_STDERR_INFO: usize = 100 * 1024; // info 模式 stderr
const MAX_STDOUT_ALL: usize = 1_000_000; // all 模式 stdout
const MAX_STDERR_ALL: usize = 500 * 1024; // all 模式 stderr
const MAX_ERROR: usize = 100 * 1024; // error/warning 模式单流限制
const MAX_WARNING_STDOUT: usize = 200 * 1024; // warning 模式 stdout（过滤后）
const DEFAULT_TIMEOUT_SECS: u64 = 30;

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

#[async_trait]
impl ITool for RunCommand {
    fn name(&self) -> &str {
        "run_command"
    }

    /// 平台感知的描述——让 LLM 知道当前执行环境，构建正确的命令。
    fn description(&self) -> &str {
        if cfg!(windows) {
            "通过 cmd /c 在 Windows 上执行 Shell 命令。\
             使用 cmd 语法：dir、del、type、set、&&、||、>、<。\
             PowerShell 请加前缀 powershell -Command \"...\"。\
             output_level 参数：'error'（仅错误）、'warning'（错误+警告）、'info'（智能摘要，默认）、'all'（完整输出）。"
        } else {
            "通过 sh -c 在 Unix（Linux/macOS）上执行 Shell 命令。\
             使用 POSIX shell 语法：ls、rm、grep、|、>、&&、$VAR。\
             脚本请显式写解释器：python3 script.py。\
             output_level 参数：'error'（仅错误）、'warning'（错误+警告）、'info'（智能摘要，默认）、'all'（完整输出）。"
        }
    }

    /// 平台感知的参数 schema。
    fn parameters(&self) -> serde_json::Value {
        let command_desc = if cfg!(windows) {
            "Shell 命令（通过 cmd /c）。使用 && 连接、> 重定向、2>&1 合并 stderr。单行字符串。"
        } else {
            "Shell 命令（通过 sh -c）。使用 && 连接、> 重定向、2>&1 合并 stderr。支持 $VAR 变量展开。单行字符串。"
        };
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": command_desc },
                "working_dir": { "type": "string", "description": "工作目录（可选；绝对路径或相对于工作区根目录的路径）。默认使用工作区根目录。" },
                "timeout_secs": { "type": "integer", "description": "超时时间（秒，可选；默认为 30 秒）。" },
                "output_level": { "type": "string", "description": "输出详细程度（可选；默认 'info'）：'error'、'warning'、'info'、'all'。" }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> rust_agent_core::Result<ToolResult> {
        self.call(arguments).await
    }

    fn kind(&self) -> &str {
        "file"
    }
}

// ── 截断工具 ──

/// 智能截断：保留尾部，因为错误摘要/构建结果通常在尾部。
/// 返回 (截断后文本, 是否截断, 原始总字节数)。
fn smart_truncate(data: &[u8], max: usize) -> (String, bool, usize) {
    let total = data.len();
    if total <= max {
        return (String::from_utf8_lossy(data).to_string(), false, total);
    }
    let tail = &data[total - max..];
    let prefix = format!("...[omitted {} bytes]\n", total - max);
    (prefix + &String::from_utf8_lossy(tail), true, total)
}

/// 硬截断（头部优先），用于非关键输出。
fn hard_truncate(data: &[u8], max: usize) -> (String, bool, usize) {
    let total = data.len();
    if total <= max {
        return (String::from_utf8_lossy(data).to_string(), false, total);
    }
    let head = String::from_utf8_lossy(&data[..max]).to_string();
    (format!("{}...[truncated, {} bytes total]", head, total), true, total)
}

/// 过滤包含 "warn" 或 "warning" 的行（大小写不敏感）。
fn filter_warning_lines(data: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(data);
    let filtered: Vec<&str> = text
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("warn")
        })
        .collect();
    filtered.join("\n").into_bytes()
}

// ── 输出级别枚举 ──

#[derive(Debug, Clone, Copy)]
enum OutputLevel {
    Error,
    Warning,
    Info,
    All,
}

impl OutputLevel {
    fn from_str(s: Option<&str>) -> Self {
        match s.unwrap_or("info") {
            "error" => OutputLevel::Error,
            "warning" => OutputLevel::Warning,
            "all" => OutputLevel::All,
            _ => OutputLevel::Info,
        }
    }
}

// ── 工作目录解析 ──

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

// ── 主实现 ──

impl RunCommand {
    async fn call(&self, arguments: serde_json::Value) -> rust_agent_core::Result<ToolResult> {
        #[derive(serde::Deserialize)]
        struct Args {
            command: String,
            working_dir: Option<String>,
            timeout_secs: Option<u64>,
            output_level: Option<String>,
        }
        let args: Args = serde_json::from_value(arguments).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Argument deserialization failed: {}",
                e
            ))
        })?;

        let output_level = OutputLevel::from_str(args.output_level.as_deref());

        let (program, shell_args) = if cfg!(windows) {
            ("cmd", vec!["/c".to_string(), args.command.clone()])
        } else {
            ("sh", vec!["-c".to_string(), args.command.clone()])
        };

        let mut cmd = Command::new(program);
        cmd.args(&shell_args);
        cmd.stdin(std::process::Stdio::null());

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

        // Scope 检测
        let scope_label = match self.scope.as_ref() {
            Some(scope) => {
                let scope_root = scope.root.as_path();
                match resolve_safe(&base_dir, cwd.to_string_lossy().as_ref(), Some(scope_root))
                {
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

        match tokio::time::timeout(
            timeout_dur,
            tokio::task::spawn_blocking(move || cmd.output()),
        )
        .await
        {
            Err(_elapsed) => Ok(ToolResult::error(format!(
                "Command execution timed out after {} seconds",
                timeout_dur.as_secs()
            ))),
            Ok(Err(join_err)) => Ok(ToolResult::error(format!(
                "Command execution failed: {}",
                join_err
            ))),
            Ok(Ok(Err(io_err))) => Ok(ToolResult::error(format!(
                "Failed to execute command: {}",
                io_err
            ))),
            Ok(Ok(Ok(output))) => {
                let exit_code = output.status.code().unwrap_or(-1);

                match output_level {
                    OutputLevel::Error => {
                        let (stderr_str, stderr_trunc, stderr_total) =
                            smart_truncate(&output.stderr, MAX_ERROR);
                        Ok(ToolResult::success(serde_json::json!({
                            "stderr": stderr_str,
                            "exit_code": exit_code,
                            "stderr_truncated": stderr_trunc,
                            "stderr_bytes_total": stderr_total,
                            "scope": scope_label,
                        })))
                    }
                    OutputLevel::Warning => {
                        let (stderr_str, stderr_trunc, stderr_total) =
                            smart_truncate(&output.stderr, MAX_ERROR);
                        let filtered = filter_warning_lines(&output.stdout);
                        let (warn_str, warn_trunc, warn_total) =
                            smart_truncate(&filtered, MAX_WARNING_STDOUT);
                        let warn_count = String::from_utf8_lossy(&filtered).lines().count();
                        Ok(ToolResult::success(serde_json::json!({
                            "stdout_warnings": warn_str,
                            "warning_count": warn_count,
                            "stderr": stderr_str,
                            "exit_code": exit_code,
                            "stdout_truncated": warn_trunc,
                            "stdout_bytes_total": warn_total,
                            "stderr_truncated": stderr_trunc,
                            "stderr_bytes_total": stderr_total,
                            "scope": scope_label,
                        })))
                    }
                    OutputLevel::Info => {
                        let (stdout_str, stdout_trunc, stdout_total) =
                            smart_truncate(&output.stdout, MAX_STDOUT_INFO);
                        let (stderr_str, stderr_trunc, stderr_total) =
                            smart_truncate(&output.stderr, MAX_STDERR_INFO);
                        let truncation_note = if stdout_trunc || stderr_trunc {
                            Some(format!(
                                "Output was truncated. Use output_level=\"error\" for errors only ({} bytes stderr), or output_level=\"all\" for full output (up to 1MB).",
                                stderr_total
                            ))
                        } else {
                            None
                        };
                        Ok(ToolResult::success(serde_json::json!({
                            "stdout": stdout_str,
                            "stderr": stderr_str,
                            "exit_code": exit_code,
                            "stdout_truncated": stdout_trunc,
                            "stdout_bytes_total": stdout_total,
                            "stderr_truncated": stderr_trunc,
                            "stderr_bytes_total": stderr_total,
                            "truncation_note": truncation_note,
                            "scope": scope_label,
                        })))
                    }
                    OutputLevel::All => {
                        let (stdout_str, stdout_trunc, stdout_total) =
                            hard_truncate(&output.stdout, MAX_STDOUT_ALL);
                        let (stderr_str, stderr_trunc, stderr_total) =
                            hard_truncate(&output.stderr, MAX_STDERR_ALL);
                        Ok(ToolResult::success(serde_json::json!({
                            "stdout": stdout_str,
                            "stderr": stderr_str,
                            "exit_code": exit_code,
                            "stdout_truncated": stdout_trunc,
                            "stdout_bytes_total": stdout_total,
                            "stderr_truncated": stderr_trunc,
                            "stderr_bytes_total": stderr_total,
                            "scope": scope_label,
                        })))
                    }
                }
            }
        }
    }
}

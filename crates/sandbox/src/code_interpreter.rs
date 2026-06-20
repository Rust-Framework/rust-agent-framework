//! `code_interpreter` 工具 — 将 LLM 调用桥接到 [`ICodeSandbox`]。

use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    ICodeSandbox, ITool, Result, SandboxLanguage, SandboxRequest, ToolResult,
};

/// MAF 对齐的代码解释器工具；后端由注入的 [`ICodeSandbox`] 决定。
pub struct CodeInterpreterTool {
    sandbox: Arc<dyn ICodeSandbox>,
    default_language: SandboxLanguage,
}

impl CodeInterpreterTool {
    pub fn new(sandbox: Arc<dyn ICodeSandbox>) -> Self {
        Self {
            sandbox,
            default_language: SandboxLanguage::python(),
        }
    }

    pub fn with_default_language(mut self, language: SandboxLanguage) -> Self {
        self.default_language = language;
        self
    }
}

#[async_trait]
impl ITool for CodeInterpreterTool {
    fn name(&self) -> &str {
        "code_interpreter"
    }

    fn description(&self) -> &str {
        "Execute code in an isolated sandbox and return stdout/stderr"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "Source code to execute" },
                "language": {
                    "type": "string",
                    "description": "Runtime language (python, javascript, shell)",
                    "default": "python"
                }
            },
            "required": ["code"]
        })
    }

    fn kind(&self) -> &str {
        "code"
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult> {
        let code = arguments
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if code.is_empty() {
            return Ok(ToolResult {
                ok: false,
                data: None,
                error: Some("missing required argument: code".into()),
            });
        }

        let language = arguments
            .get("language")
            .and_then(|v| v.as_str())
            .map(|s| SandboxLanguage(s.to_string()))
            .unwrap_or_else(|| self.default_language.clone());

        let result = self
            .sandbox
            .execute(SandboxRequest {
                language,
                code,
                timeout: None,
                workspace_root: None,
                input: arguments.get("input").cloned(),
            })
            .await?;

        let payload = serde_json::json!({
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
            "backend": self.sandbox.backend_name(),
        });

        Ok(ToolResult {
            ok: result.ok(),
            data: Some(payload),
            error: if result.ok() {
                None
            } else {
                Some(result.stderr.clone())
            },
        })
    }
}

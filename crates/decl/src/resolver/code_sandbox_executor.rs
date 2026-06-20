//! ExecuteCode 工作流执行器 — 直接调用 [`ICodeSandbox`]。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{ICodeSandbox, Result, SandboxLanguage, SandboxRequest};
use rust_agent_workflow::engine::IWorkflowContext;
use rust_agent_workflow::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use tokio::sync::mpsc::UnboundedSender;

pub struct CodeSandboxExecutor {
    id: String,
    sandbox: Arc<dyn ICodeSandbox>,
    code: String,
    language: SandboxLanguage,
    output_variable: Option<String>,
}

impl CodeSandboxExecutor {
    pub fn new(
        id: impl Into<String>,
        sandbox: Arc<dyn ICodeSandbox>,
        code: impl Into<String>,
        language: SandboxLanguage,
        output_variable: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            sandbox,
            code: code.into(),
            language,
            output_variable,
        }
    }
}

#[async_trait]
impl IExecutor for CodeSandboxExecutor {
    fn id(&self) -> &str {
        &self.id
    }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("ChatMessage")]
    }

    fn send_types(&self) -> Vec<TypeTag> {
        vec![TypeTag::new("String")]
    }

    fn is_output(&self) -> bool {
        true
    }

    async fn handle(
        &self,
        _message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let result = self
            .sandbox
            .execute(SandboxRequest {
                language: self.language.clone(),
                code: self.code.clone(),
                timeout: None,
                workspace_root: None,
                input: None,
            })
            .await?;

        let payload = serde_json::json!({
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
            "backend": self.sandbox.backend_name(),
            "ok": result.ok(),
        });

        let text = result.stdout.clone();
        let _ = progress.send(NodeProgress::TextDelta(text.clone()));

        if let Some(ref key) = self.output_variable {
            ctx.write_state(key, payload.clone()).await?;
        }
        ctx.write_state("__code_result", payload).await?;

        Ok(HandlerResult::Messages(vec![Arc::new(text)]))
    }
}

/// 从 ActionDecl 的 code 字段解析源码字符串。
pub fn resolve_code_literal(code: &serde_json::Value) -> String {
    match code {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 合并工作流级与动作级沙箱配置。
pub fn merge_sandbox_config(
    defaults: &HashMap<String, serde_json::Value>,
    overrides: &HashMap<String, serde_json::Value>,
) -> HashMap<String, serde_json::Value> {
    let mut out = defaults.clone();
    for (k, v) in overrides {
        out.insert(k.clone(), v.clone());
    }
    out
}

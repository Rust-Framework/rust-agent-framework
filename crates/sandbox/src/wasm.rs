//! WASM 沙箱 — wasmtime 后端（feature `wasm`）或占位实现。

use async_trait::async_trait;
use rust_agent_core::{AgentError, ICodeSandbox, Result, SandboxRequest, SandboxResult};

/// WASM 沙箱后端。
pub struct WasmSandbox {
    #[cfg(feature = "wasm")]
    export_name: String,
}

impl WasmSandbox {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "wasm")]
            export_name: "run".into(),
        }
    }

    #[cfg(feature = "wasm")]
    pub fn with_export_name(mut self, name: impl Into<String>) -> Self {
        self.export_name = name.into();
        self
    }
}

impl Default for WasmSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "wasm")]
impl WasmSandbox {
    fn decode_module(request: &SandboxRequest) -> Result<Vec<u8>> {
        use base64::Engine as _;
        match request.language.0.as_str() {
            "wat" => wat::parse_str(&request.code)
                .map_err(|e| AgentError::ConfigError(format!("invalid WAT: {e}"))),
            "wasm" => base64::engine::general_purpose::STANDARD
                .decode(request.code.trim())
                .map_err(|e| AgentError::ConfigError(format!("invalid wasm base64: {e}"))),
            other => Err(AgentError::ConfigError(format!(
                "WasmSandbox expects language 'wat' or 'wasm', got '{other}'"
            ))),
        }
    }

    fn run_sync(bytes: &[u8], export_name: &str, input: Option<i64>) -> Result<SandboxResult> {
        use wasmtime::*;

        let engine = Engine::default();
        let module = Module::new(&engine, bytes)
            .map_err(|e| AgentError::ToolError(format!("wasm module load: {e}")))?;
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[])
            .map_err(|e| AgentError::ToolError(format!("wasm instantiate: {e}")))?;

        if let Some(arg) = input {
            for name in [export_name, "run", "main"] {
                if let Ok(func) = instance.get_typed_func::<i32, i32>(&mut store, name) {
                    let value = func
                        .call(&mut store, arg as i32)
                        .map_err(|e| AgentError::ToolError(format!("wasm call {name}: {e}")))?;
                    return Ok(SandboxResult::success(value.to_string()));
                }
            }
        }

        for name in [export_name, "run", "main", "_start"] {
            if let Ok(func) = instance.get_typed_func::<(), i32>(&mut store, name) {
                let value = func
                    .call(&mut store, ())
                    .map_err(|e| AgentError::ToolError(format!("wasm call {name}: {e}")))?;
                return Ok(SandboxResult::success(value.to_string()));
            }
        }

        Err(AgentError::ToolError(format!(
            "wasm module has no callable export (tried '{export_name}', run, main, _start)"
        )))
    }

    fn input_as_i64(input: &Option<serde_json::Value>) -> Option<i64> {
        let value = input.as_ref()?;
        value
            .as_i64()
            .or_else(|| value.get("value").and_then(|v| v.as_i64()))
    }
}

#[async_trait]
impl ICodeSandbox for WasmSandbox {
    async fn execute(&self, request: SandboxRequest) -> Result<SandboxResult> {
        #[cfg(feature = "wasm")]
        {
            let bytes = Self::decode_module(&request)?;
            let export = self.export_name.clone();
            let input = Self::input_as_i64(&request.input);
            return tokio::task::spawn_blocking(move || Self::run_sync(&bytes, &export, input))
                .await
                .map_err(|e| AgentError::ToolError(format!("wasm task: {e}")))?;
        }

        #[cfg(not(feature = "wasm"))]
        {
            let _ = request;
            Err(AgentError::ConfigError(
                "WasmSandbox requires rust-agent-sandbox `wasm` feature (wasmtime)".into(),
            ))
        }
    }

    fn backend_name(&self) -> &str {
        "wasm"
    }
}

#[cfg(all(test, feature = "wasm"))]
mod tests {
    use super::*;
    use rust_agent_core::SandboxLanguage;

    #[tokio::test]
    async fn runs_wat_module() {
        let sandbox = WasmSandbox::new();
        let result = sandbox
            .execute(SandboxRequest {
                language: SandboxLanguage("wat".into()),
                code: r#"(module (func (export "run") (result i32) i32.const 42))"#.into(),
                timeout: None,
                workspace_root: None,
                input: None,
            })
            .await
            .expect("execute");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "42");
    }

    #[tokio::test]
    async fn runs_wat_with_input_arg() {
        let sandbox = WasmSandbox::new();
        let result = sandbox
            .execute(SandboxRequest {
                language: SandboxLanguage("wat".into()),
                code: r#"(module (func (export "run") (param i32) (result i32) local.get 0))"#
                    .into(),
                timeout: None,
                workspace_root: None,
                input: Some(serde_json::json!(7)),
            })
            .await
            .expect("execute");
        assert_eq!(result.stdout.trim(), "7");
    }
}

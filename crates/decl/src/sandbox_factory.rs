//! 从声明式 config 构建 [`ICodeSandbox`] 后端。

use std::collections::HashMap;
use std::sync::Arc;

use crate::error::DeclError;

/// 解析 YAML `config` 并实例化沙箱后端。
pub fn build_sandbox(
    config: &HashMap<String, serde_json::Value>,
) -> crate::Result<Arc<dyn rust_agent_core::ICodeSandbox>> {
    #[cfg(feature = "sandbox")]
    {
        return build_sandbox_impl(config);
    }
    #[cfg(not(feature = "sandbox"))]
    {
        let _ = config;
        Err(DeclError::Unsupported(
            "sandbox backends require decl `sandbox` feature".into(),
        ))
    }
}

/// 构建 `code_interpreter` 工具。
pub fn build_code_interpreter(
    config: &HashMap<String, serde_json::Value>,
) -> crate::Result<Arc<dyn rust_agent_core::ITool>> {
    #[cfg(feature = "sandbox")]
    {
        let sandbox = build_sandbox_impl(config)?;
        let mut tool = rust_agent_sandbox::CodeInterpreterTool::new(sandbox);
        tool = tool.with_default_language(default_language(config));
        return Ok(Arc::new(tool));
    }
    #[cfg(not(feature = "sandbox"))]
    {
        let _ = config;
        Err(DeclError::Unsupported(
            "code_interpreter requires decl `sandbox` feature".into(),
        ))
    }
}

pub fn default_language(
    config: &HashMap<String, serde_json::Value>,
) -> rust_agent_core::SandboxLanguage {
    config
        .get("default_language")
        .or_else(|| config.get("language"))
        .and_then(|v| v.as_str())
        .map(|s| rust_agent_core::SandboxLanguage(s.to_string()))
        .unwrap_or_else(rust_agent_core::SandboxLanguage::python)
}

#[cfg(feature = "sandbox")]
fn build_sandbox_impl(
    config: &HashMap<String, serde_json::Value>,
) -> crate::Result<Arc<dyn rust_agent_core::ICodeSandbox>> {
    use std::time::Duration;

    let backend = config
        .get("backend")
        .and_then(|v| v.as_str())
        .unwrap_or("process");

    match backend {
        "process" => {
            let mut sb = rust_agent_sandbox::ProcessSandbox::new();
            if let Some(secs) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
                sb = sb.with_timeout(Duration::from_secs(secs));
            }
            Ok(Arc::new(sb))
        }
        "container" => {
            let mut sb = rust_agent_sandbox::ContainerSandbox::new();
            if let Some(secs) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
                sb = sb.with_timeout(Duration::from_secs(secs));
            }
            Ok(Arc::new(sb))
        }
        "docker" => build_container_cli(config, "docker"),
        "podman" => build_container_cli(config, "podman"),
        "wasm" => build_wasm(config),
        other => Err(DeclError::Unsupported(format!(
            "unknown sandbox backend '{other}' — use process, container, docker, podman, or wasm"
        ))),
    }
}

#[cfg(feature = "sandbox")]
fn build_container_cli(
    config: &HashMap<String, serde_json::Value>,
    cli: &str,
) -> crate::Result<Arc<dyn rust_agent_core::ICodeSandbox>> {
    #[cfg(feature = "sandbox-docker")]
    {
        use std::time::Duration;

        let mut sb = rust_agent_sandbox::DockerSandbox::new().with_cli(cli);
        if let Some(v) = config.get("network").and_then(|v| v.as_bool()) {
            sb = sb.with_network(v);
        }
        if let Some(v) = config.get("memory_limit").and_then(|v| v.as_str()) {
            sb = sb.with_memory_limit(v);
        }
        if let Some(v) = config.get("python_image").and_then(|v| v.as_str()) {
            sb = sb.with_python_image(v);
        }
        if let Some(v) = config.get("node_image").and_then(|v| v.as_str()) {
            sb = sb.with_node_image(v);
        }
        if let Some(v) = config.get("cpus").and_then(|v| v.as_str()) {
            sb = sb.with_cpus(v);
        }
        if let Some(v) = config.get("pids_limit").and_then(|v| v.as_u64()) {
            sb = sb.with_pids_limit(v);
        }
        if let Some(secs) = config.get("timeout_secs").and_then(|v| v.as_u64()) {
            sb = sb.with_timeout(Duration::from_secs(secs));
        }
        Ok(Arc::new(sb))
    }
    #[cfg(not(feature = "sandbox-docker"))]
    {
        let _ = config;
        Err(DeclError::Unsupported(format!(
            "sandbox backend '{cli}' requires decl feature `sandbox-docker`"
        )))
    }
}

#[cfg(feature = "sandbox")]
fn build_wasm(
    config: &HashMap<String, serde_json::Value>,
) -> crate::Result<Arc<dyn rust_agent_core::ICodeSandbox>> {
    #[cfg(feature = "sandbox-wasm")]
    {
        let mut sb = rust_agent_sandbox::WasmSandbox::new();
        if let Some(name) = config.get("export").and_then(|v| v.as_str()) {
            sb = sb.with_export_name(name);
        }
        Ok(Arc::new(sb))
    }
    #[cfg(not(feature = "sandbox-wasm"))]
    {
        let _ = config;
        Err(DeclError::Unsupported(
            "sandbox backend 'wasm' requires decl feature `sandbox-wasm`".into(),
        ))
    }
}

//! # rust-agent-openapi
//!
//! 从 OpenAPI 3.x 规范解析并实例化 HTTP 工具 — 独立 crate，不污染 core/decl 默认依赖。

mod spec;
mod tool;
mod validate;

use std::sync::Arc;

use rust_agent_core::{AgentError, ITool, Result};

pub use spec::{
    build_request_url, parse_spec, resolve_schema, resolve_value_ref, OpenApiSpec,
    ResolvedOperation, ResolvedParameter, SecurityScheme,
};
pub use validate::validate_response_body;
pub use tool::{tool_from_spec_json, OpenApiHttpTool};

/// OpenAPI 工具解析器配置。
#[derive(Debug, Clone)]
pub struct OpenApiToolConfig {
    pub spec_url: String,
    pub operation_id: Option<String>,
    pub base_url: Option<String>,
    pub tool_name: String,
}

/// 从 OpenAPI 规范 URL 解析并返回 `ITool`。
pub struct OpenApiToolResolver;

impl OpenApiToolResolver {
    pub async fn resolve(config: &OpenApiToolConfig) -> Result<Arc<dyn ITool>> {
        let spec_text = fetch_spec(&config.spec_url).await?;
        tool_from_spec_json(
            &config.tool_name,
            &spec_text,
            config.operation_id.as_deref(),
            config.base_url.as_deref(),
        )
    }

    pub fn resolve_from_str(
        config: &OpenApiToolConfig,
        spec_json: &str,
    ) -> Result<Arc<dyn ITool>> {
        tool_from_spec_json(
            &config.tool_name,
            spec_json,
            config.operation_id.as_deref(),
            config.base_url.as_deref(),
        )
    }
}

async fn fetch_spec(url: &str) -> Result<String> {
    if url.starts_with("file://") {
        let path = url.trim_start_matches("file://");
        return tokio::fs::read_to_string(path)
            .await
            .map_err(|e| AgentError::ConfigError(format!("read OpenAPI file: {e}")));
    }
    reqwest::get(url)
        .await
        .map_err(|e| AgentError::ConfigError(format!("fetch OpenAPI spec: {e}")))?
        .text()
        .await
        .map_err(|e| AgentError::ConfigError(format!("read OpenAPI spec: {e}")))
}

/// 向后兼容占位类型。
pub struct OpenApiTool {
    inner: OpenApiHttpTool,
}

impl OpenApiTool {
    pub fn from_http(tool: OpenApiHttpTool) -> Self {
        Self { inner: tool }
    }
}

#[async_trait::async_trait]
impl ITool for OpenApiTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters(&self) -> serde_json::Value {
        self.inner.parameters()
    }

    fn kind(&self) -> &str {
        "openapi"
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<rust_agent_core::ToolResult> {
        self.inner.execute(arguments).await
    }
}

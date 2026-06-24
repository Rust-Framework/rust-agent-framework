//! 工作流编译注册表 — 替代 deprecated `AgentResolver` 的轻量注册中心。

use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{IAgent, ITool};
#[cfg(feature = "mcp")]
use rust_agent_mcp::McpServerClient;

use crate::resolver::tool_resolver::ToolResolver;

/// 工作流编译期资源注册表（Agent / Tool / MCP）。
#[derive(Default)]
pub struct CompileRegistry {
    agents: Vec<(String, Arc<dyn IAgent>)>,
    tool_resolver: ToolResolver,
    #[cfg(feature = "mcp")]
    mcp_servers: HashMap<String, Arc<McpServerClient>>,
    /// 工作流/Agent 级沙箱默认配置
    sandbox_defaults: HashMap<String, serde_json::Value>,
    /// 预解析的工具缓存（functionName → tool）
    tools: HashMap<String, Arc<dyn ITool>>,
}

impl CompileRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tool_resolver_mut(&mut self) -> &mut ToolResolver {
        &mut self.tool_resolver
    }

    pub fn set_sandbox_defaults(&mut self, defaults: HashMap<String, serde_json::Value>) {
        self.sandbox_defaults = defaults.clone();
        self.tool_resolver.set_sandbox_defaults(defaults);
    }

    pub fn sandbox_defaults(&self) -> &HashMap<String, serde_json::Value> {
        &self.sandbox_defaults
    }

    pub fn register_agent(&mut self, name: impl Into<String>, agent: Arc<dyn IAgent>) {
        self.agents.push((name.into(), agent));
    }

    pub fn get_agent(&self, name: &str) -> Option<Arc<dyn IAgent>> {
        self.agents
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, a)| Arc::clone(a))
    }

    #[cfg(feature = "mcp")]
    pub fn register_mcp_server(
        &mut self,
        server_url: impl Into<String>,
        server: Arc<McpServerClient>,
    ) {
        let url = server_url.into();
        self.tool_resolver
            .register_mcp_server_arc(url.clone(), Arc::clone(&server));
        self.mcp_servers.insert(url, server);
    }

    #[cfg(feature = "mcp")]
    pub fn get_mcp_server(&self, server_url: &str) -> Option<&Arc<McpServerClient>> {
        self.mcp_servers.get(server_url)
    }

    pub fn register_tool_factory(
        &mut self,
        name: impl Into<String>,
        factory: impl Fn(HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>>
            + Send
            + Sync
            + 'static,
    ) {
        self.tool_resolver.register_factory(name, factory);
    }

    /// 注册已解析工具，供 InvokeFunctionTool 按 functionName 查找。
    pub fn register_tool(&mut self, name: impl Into<String>, tool: Arc<dyn ITool>) {
        self.tools.insert(name.into(), tool);
    }

    pub fn get_tool(&self, name: &str) -> Option<Arc<dyn ITool>> {
        self.tools.get(name).cloned()
    }

    /// 按 functionName 解析工具（内置名 → 工厂 → 缓存）。
    pub async fn resolve_tool(&mut self, name: &str) -> crate::Result<Arc<dyn ITool>> {
        if let Some(t) = self.tools.get(name) {
            return Ok(Arc::clone(t));
        }
        let tool = self.tool_resolver.resolve_by_function_name(name).await?;
        self.tools.insert(name.to_string(), Arc::clone(&tool));
        Ok(tool)
    }
}

//! DeclAgentBuilder — 声明式 Agent 构建器
//!
//! 对标 `AgentBuilder`（框架层），将 YAML/JSON/TOML 声明文件
//! 加载为可执行的 `Arc<dyn IAgent>`。
//!
//! ## 最小示例
//!
//! ```ignore
//! use rust_agent_decl::DeclAgentBuilder;
//!
//! let agent = DeclAgentBuilder::new()
//!     .from_yaml_file("my-agent.yaml")
//!     .with_model("deepseek-v4-flash")
//!     .with_api_key(&std::env::var("DEEPSEEK_API_KEY").unwrap())
//!     .build()
//!     .await?;
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::{IAgent, IContextProvider, ITool};
use rust_agent_framework::AgentBuilder;

use crate::connection::{ConnectionDetails, ConnectionKind};
use crate::definition::AgentKindData;
use crate::document::AgentDocument;
use crate::error::{DeclError, Result};
use crate::ext::{ChatClientWrapper, ToolWrapper};
use crate::resolver::connection_resolver;

/// 工具工厂回调类型。
pub type ToolFactoryCallback = Arc<
    dyn Fn(HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>>
        + Send + Sync,
>;

/// 声明式 Agent 构建器。
///
/// 从 MAF v1.0 兼容的 YAML/JSON/TOML 文件加载 Agent 定义，
/// 支持运行时覆盖模型、API Key、工具工厂和上下文提供器。
pub struct DeclAgentBuilder {
    yaml_path: Option<PathBuf>,
    yaml_str: Option<String>,
    model_id: Option<String>,
    api_key: Option<String>,
    tool_factories: Vec<(String, ToolFactoryCallback)>,
    /// 代码注入的上下文提供器（通过 with_context() 添加）。
    external_contexts: Vec<Arc<dyn IContextProvider>>,
    max_tool_rounds: Option<usize>,
}

impl DeclAgentBuilder {
    /// 创建空的构建器。
    pub fn new() -> Self {
        Self {
            yaml_path: None,
            yaml_str: None,
            model_id: None,
            api_key: None,
            tool_factories: Vec::new(),
            external_contexts: Vec::new(),
            max_tool_rounds: None,
        }
    }

    /// 从 YAML 文件加载声明（相对路径基于当前工作目录）。
    pub fn from_yaml_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.yaml_path = Some(path.into());
        self
    }

    /// 从字符串加载 YAML 声明。
    pub fn from_yaml_str(mut self, yaml: &str) -> Self {
        self.yaml_str = Some(yaml.to_string());
        self
    }

    /// 覆盖 YAML 中的 model.id。
    pub fn with_model(mut self, model_id: &str) -> Self {
        self.model_id = Some(model_id.to_string());
        self
    }

    /// 设置 API Key（覆盖 YAML 中的 `$VAR` 占位符）。
    pub fn with_api_key(mut self, api_key: &str) -> Self {
        self.api_key = Some(api_key.to_string());
        self
    }

    /// 注册工具工厂。YAML 中 `tools: [{kind: function, name: "xxx"}]` 声明的
    /// 工具通过此工厂闭包实例化为 `Arc<dyn ITool>`。
    pub fn with_tool(
        mut self,
        name: &str,
        factory: impl Fn(HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.tool_factories
            .push((name.to_string(), Arc::new(factory)));
        self
    }

    /// 注入 ContextProvider 外挂。声明式上下文中未覆盖的 provider
    /// 通过此方法在运行时注入，与 YAML 中 `contexts` 声明的 provider 合并。
    pub fn with_context(mut self, provider: Arc<dyn IContextProvider>) -> Self {
        self.external_contexts.push(provider);
        self
    }

    /// 设置最大工具调用轮次。
    pub fn max_tool_rounds(mut self, rounds: usize) -> Self {
        self.max_tool_rounds = Some(rounds);
        self
    }

    /// 加载声明并构建 Agent。
    pub async fn build(self) -> Result<Arc<dyn IAgent>> {
        // 1. 解析 YAML
        let doc = self.load_yaml()?;

        let mut def = doc.into_definition().ok_or_else(|| {
            DeclError::Validation("AgentDocument contains a Manifest; expected a bare AgentDefinition".into())
        })?;

        // 2. 运行时覆盖模型和 API Key
        {
            let prompt_data = match &mut def.kind_data {
                AgentKindData::Prompt(data) => data,
                _ => {
                    return Err(DeclError::Unsupported(
                        "DeclAgentBuilder only supports 'kind: prompt' agents".into(),
                    ));
                }
            };

            if let Some(ref mid) = self.model_id {
                prompt_data.model.id = mid.clone();
            }

            if let Some(ref key) = self.api_key {
                prompt_data.model.connection = Some(crate::connection::Connection {
                    kind: ConnectionKind::ApiKey,
                    authentication_mode: crate::connection::AuthenticationMode::System,
                    usage_description: None,
                    details: ConnectionDetails {
                        api_key: Some(key.clone()),
                        ..Default::default()
                    },
                });
            }
        }

        let prompt_data = match &def.kind_data {
            AgentKindData::Prompt(data) => data,
            _ => return Err(DeclError::Unsupported("Expected prompt agent".into())),
        };

        // 3. 构建声明式上下文提供器（从 YAML contexts 段）
        let mut all_context_providers: Vec<Arc<dyn IContextProvider>> = Vec::new();

        // 3a. 从 YAML 声明构建
        for decl in &prompt_data.contexts {
            if let Some(provider) = self.build_provider_from_decl(decl) {
                all_context_providers.push(provider);
            }
        }

        // 3b. 合并代码注入的 provider（with_context()）
        all_context_providers.extend(self.external_contexts.iter().map(Arc::clone));

        // 4. 解析 chat_client
        let chat_client = connection_resolver::resolve_chat_client(&prompt_data.model)?;
        let instructions = prompt_data.instructions.clone();
        let tools_list = prompt_data.tools.clone();

        // 5. 通过 AgentBuilder 统一构建（始终走 context_provider 路径）
        let mut builder = AgentBuilder::new(&def.name)
            .chat_client(ChatClientWrapper(chat_client))
            .instructions(&instructions);

        if !def.description.is_empty() {
            builder = builder.with_description(&def.description);
        }

        for cp in &all_context_providers {
            builder = builder.add_context_provider_shared(Arc::clone(cp));
        }

        // 6. 解析工具
        let mut tool_resolver = crate::resolver::tool_resolver::ToolResolver::new();
        for (name, factory) in &self.tool_factories {
            let factory = Arc::clone(factory);
            tool_resolver.register_factory(name, move |args| factory(args));
        }
        let tools = tool_resolver.resolve_all(&tools_list).await?;
        for tool in tools {
            builder = builder.with_tool(ToolWrapper(tool));
        }

        if let Some(rounds) = self.max_tool_rounds {
            builder = builder.max_tool_rounds(rounds);
        }

        Ok(builder.build()?)
    }

    /// 从声明式配置构建单个上下文提供器。
    fn build_provider_from_decl(
        &self,
        decl: &crate::context_provider_config::ContextProviderDecl,
    ) -> Option<Arc<dyn IContextProvider>> {
        use crate::context_provider_config::ContextProviderDecl;

        match decl {
            // ── memory ──
            ContextProviderDecl::Memory { name, config } if name == "skill-memory" => {
                let dir = config
                    .get("directory")
                    .and_then(|v| v.as_str())
                    .unwrap_or("logs/memory");
                let enabled = config
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let interval = config
                    .get("consolidationInterval")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(3) as usize;

                let memory_dir = std::path::PathBuf::from(dir);
                std::fs::create_dir_all(&memory_dir).ok();

                let sm = rust_agent_framework::memory::SkillMemoryContextProvider::new(&memory_dir)
                    .with_enabled(enabled)
                    .with_consolidation_interval(interval);
                Some(Arc::new(sm))
            }

            // ── skills ──
            ContextProviderDecl::Skills { name: _skill_name, config } => {
                let dir = config
                    .get("directory")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tracing::warn!(
                    "Skills declarative provider not yet implemented; skill: {}, directory: {}",
                    _skill_name, dir
                );
                None
            }

            // ── mcp ──
            ContextProviderDecl::Mcp { name: _server_name, config } => {
                let server_url = config
                    .get("serverUrl")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tracing::warn!(
                    "MCP declarative provider not yet implemented; server: {}, url: {}",
                    _server_name, server_url
                );
                None
            }

            // ── workspace ──
            ContextProviderDecl::Workspace { name: _ws_name, config } => {
                let root = config
                    .get("root")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                let _policy = config
                    .get("policy")
                    .and_then(|v| v.as_str())
                    .unwrap_or("read");
                tracing::warn!(
                    "Workspace declarative provider not yet implemented; name: {}, root: {}",
                    _ws_name, root
                );
                None
            }

            // ── knowledge (RAG) ──
            ContextProviderDecl::Knowledge { name: _kb_name, config } => {
                let source = config
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tracing::warn!(
                    "Knowledge (RAG) declarative provider not yet implemented; name: {}, source: {}",
                    _kb_name, source
                );
                None
            }

            // ── wiki ──
            ContextProviderDecl::Wiki { name: _wiki_name, config } => {
                let source = config
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tracing::warn!(
                    "Wiki declarative provider not yet implemented; name: {}, source: {}",
                    _wiki_name, source
                );
                None
            }

            ContextProviderDecl::Memory { .. } => {
                tracing::debug!("Unknown memory provider name (expected 'skill-memory')");
                None
            }
        }
    }

    #[cfg(feature = "yaml")]
    fn load_yaml(&self) -> Result<AgentDocument> {
        if let Some(ref path) = self.yaml_path {
            AgentDocument::from_yaml_file(&path.to_string_lossy()).map_err(|e| {
                DeclError::Resolution(format!(
                    "Failed to load YAML file '{}': {}",
                    path.display(),
                    e
                ))
            })
        } else if let Some(ref yaml) = self.yaml_str {
            AgentDocument::from_yaml_str(yaml)
        } else {
            Err(DeclError::Validation(
                "DeclAgentBuilder requires a YAML source (.from_yaml_file() or .from_yaml_str())".into(),
            ))
        }
    }

    #[cfg(not(feature = "yaml"))]
    fn load_yaml(&self) -> Result<AgentDocument> {
        Err(DeclError::Unsupported(
            "YAML feature is required. Enable 'yaml' in rust-agent-decl".into(),
        ))
    }
}

impl Default for DeclAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

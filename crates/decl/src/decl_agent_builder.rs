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
//!     .with_model("agnes-2.0-flash")
//!     .with_api_key(&std::env::var("DEEPSEEK_API_KEY").unwrap())
//!     .build()
//!     .await?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rust_agent_core::{IAgent, IContextProvider, ITool};
use rust_agent_framework::AgentBuilder;

use crate::connection::{ConnectionDetails, ConnectionKind};
use crate::definition::AgentKindData;
use crate::document::AgentDocument;
use crate::error::{DeclError, Result};
use crate::ext::{ChatClientWrapper, ToolWrapper};
use crate::resolver::connection_resolver;

/// 声明源 — 统一 YAML/JSON/TOML 文件或字符串。
enum Source {
    File(PathBuf),
    Str(String),
}

/// 工具工厂回调类型。
pub type ToolFactoryCallback = Arc<
    dyn Fn(HashMap<String, serde_json::Value>) -> crate::Result<Arc<dyn ITool>>
        + Send + Sync,
>;

/// 声明式 Agent 构建器 — RAF 推荐入口。
///
/// 从 MAF v1.0 兼容的 YAML/JSON/TOML 文件加载 Agent 定义，
/// 支持运行时覆盖模型、API Key、工具工厂和上下文提供器。
///
/// # 三种使用路径
///
/// ```ignore
/// // 极简路径 — 一行代码
/// let agent = DeclAgentBuilder::quick("agent.yaml").await?;
///
/// // 标准路径 — 运行时覆盖
/// let agent = DeclAgentBuilder::from_file("agent.yaml")
///     .with_api_key(&env::var("DEEPSEEK_API_KEY").unwrap())
///     .build().await?;
///
/// // 高级路径 — 从内存定义构建
/// let agent = DeclAgentBuilder::from_definition(agent_def)
///     .with_context(custom_provider)
///     .build().await?;
/// ```
pub struct DeclAgentBuilder {
    source: Option<Source>,
    model_id: Option<String>,
    api_key: Option<String>,
    tool_factories: Vec<(String, ToolFactoryCallback)>,
    /// 代码注入的上下文提供器（通过 with_context() 添加）。
    external_contexts: Vec<Arc<dyn IContextProvider>>,
    max_tool_rounds: Option<usize>,
    /// `kind: workflow` 中 InvokeAgent 引用的预注册 Agent。
    workflow_agents: HashMap<String, Arc<dyn IAgent>>,
    /// 命名连接注册表（`kind: reference` 解析）。
    connections: HashMap<String, crate::connection::Connection>,
    /// 从 AgentDefinition 直接构建（escape hatch，不经过文件解析）。
    direct_definition: Option<crate::definition::AgentDefinition>,
}

impl DeclAgentBuilder {
    /// 创建空的构建器。
    pub fn new() -> Self {
        Self {
            source: None,
            model_id: None,
            api_key: None,
            tool_factories: Vec::new(),
            external_contexts: Vec::new(),
            max_tool_rounds: None,
            workflow_agents: HashMap::new(),
            connections: HashMap::new(),
            direct_definition: None,
        }
    }

    // ── 快速路径 ──

    /// 极简入口：从文件加载并构建 Agent（YAML/JSON/TOML 自动检测）。
    ///
    /// 相当于 `DeclAgentBuilder::from_file(path).build().await`。
    pub async fn quick(path: impl AsRef<Path>) -> Result<Arc<dyn IAgent>> {
        Self::new().from_file(path).build().await
    }

    // ── 文件加载路径 ──

    /// 从文件加载声明（根据扩展名自动选择解析器）。
    ///
    /// - `.yaml` / `.yml` → YAML（需启用 `yaml` feature）
    /// - `.json` → JSON
    /// - `.toml` → TOML（需启用 `toml` feature）
    pub fn from_file(mut self, path: impl AsRef<Path>) -> Self {
        self.source = Some(Source::File(path.as_ref().to_path_buf()));
        self
    }

    /// 从 YAML 文件加载声明（相对路径基于当前工作目录）。
    pub fn from_yaml_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.source = Some(Source::File(path.into()));
        self
    }

    /// 从 JSON 文件加载声明。
    pub fn from_json_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.source = Some(Source::File(path.into()));
        self
    }

    /// 从 TOML 文件加载声明。
    pub fn from_toml_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.source = Some(Source::File(path.into()));
        self
    }

    /// 从 YAML 字符串加载声明。
    pub fn from_yaml_str(mut self, yaml: &str) -> Self {
        self.source = Some(Source::Str(yaml.to_string()));
        self
    }

    /// 从 JSON 字符串加载声明。
    pub fn from_json_str(mut self, json: &str) -> Self {
        self.source = Some(Source::Str(json.to_string()));
        self
    }

    /// 从 TOML 字符串加载声明。
    pub fn from_toml_str(mut self, toml: &str) -> Self {
        self.source = Some(Source::Str(toml.to_string()));
        self
    }

    // ── 高级路径 ──

    /// 从已解析的 `AgentDefinition` 构建（escape hatch）。
    ///
    /// 适用于需要程序化修改定义后再构建的场景。
    pub fn from_definition(mut self, def: crate::definition::AgentDefinition) -> Self {
        self.direct_definition = Some(def);
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

    /// 注册 workflow 图中 `InvokeAgent` 可引用的 Agent。
    pub fn with_workflow_agent(
        mut self,
        name: impl Into<String>,
        agent: Arc<dyn IAgent>,
    ) -> Self {
        self.workflow_agents.insert(name.into(), agent);
        self
    }

    /// 注册命名连接（供 model.connection `kind: reference` 引用）。
    pub fn with_connection(
        mut self,
        name: impl Into<String>,
        connection: crate::connection::Connection,
    ) -> Self {
        self.connections.insert(name.into(), connection);
        self
    }

    /// 设置最大工具调用轮次。
    pub fn max_tool_rounds(mut self, rounds: usize) -> Self {
        self.max_tool_rounds = Some(rounds);
        self
    }

    // ── 验证 ──

    /// 在不启动 LLM 的情况下验证配置的完整性和正确性。
    ///
    /// 返回 `ValidationReport`，包含错误、警告、已解析的工具和提供器列表。
    pub async fn validate(self) -> Result<ValidationReport> {
        let mut report = ValidationReport::default();

        let def = if let Some(ref direct_def) = self.direct_definition {
            direct_def.clone()
        } else {
            match self.load_document() {
                Ok(doc) => match doc.into_definition() {
                    Some(d) => d,
                    None => {
                        report.errors.push(
                            "Document is an AgentManifest; expected a bare AgentDefinition. \
                             Remove the outer 'template' wrapper."
                                .into(),
                        );
                        return Ok(report);
                    }
                },
                Err(e) => {
                    report.errors.push(format!("Failed to parse config: {}", e));
                    return Ok(report);
                }
            }
        };

        let prompt_data = match &def.kind_data {
            AgentKindData::Prompt(data) => data,
            _ => {
                report
                    .errors
                    .push("Only 'kind: prompt' agents are supported by DeclAgentBuilder".into());
                return Ok(report);
            }
        };

        // 验证模型
        if prompt_data.model.id.is_empty() {
            report.errors.push("model.id is required but empty.".into());
        } else {
            report.resolved_model = Some(prompt_data.model.id.clone());
        }

        // 验证工具
        let mut tool_resolver = crate::resolver::tool_resolver::ToolResolver::new();
        for (name, factory) in &self.tool_factories {
            let factory = Arc::clone(factory);
            tool_resolver.register_factory(name, move |args| factory(args));
        }

        match tool_resolver.resolve_all(&prompt_data.tools).await {
            Ok(tools) => {
                for tool in tools {
                    report.resolved_tools.push(format!(
                        "{} (description from code)",
                        tool.name()
                    ));
                }
            }
            Err(e) => {
                report.errors.push(format!("Failed to resolve tools: {}", e));
            }
        }

        for sub_def in &prompt_data.sub_agents {
            let sub_prompt = match &sub_def.kind_data {
                AgentKindData::Prompt(data) => data,
                _ => {
                    report.errors.push(format!(
                        "Sub-agent '{}' must be kind: prompt",
                        sub_def.name
                    ));
                    continue;
                }
            };
            match tool_resolver.resolve_all(&sub_prompt.tools).await {
                Ok(tools) => {
                    for tool in tools {
                        report.resolved_tools.push(format!(
                            "{} (sub-agent={}, description from code)",
                            tool.name(),
                            sub_def.name
                        ));
                    }
                }
                Err(e) => {
                    report.errors.push(format!(
                        "Sub-agent '{}' tool resolution failed: {}",
                        sub_def.name, e
                    ));
                }
            }
        }

        if !prompt_data.sub_agents.is_empty() {
            report.resolved_providers.push(format!(
                "orchestration(magentic, {} sub-agents)",
                prompt_data.sub_agents.len()
            ));
        }

        // 验证 context providers
        for ctx_decl in &prompt_data.contexts {
            let ctx_kind = match ctx_decl {
                crate::context_provider_config::ContextProviderDecl::Bundle { name, .. } => {
                    format!("bundle({})", name)
                }
                crate::context_provider_config::ContextProviderDecl::Skills { name, .. } => {
                    format!("skills({})", name)
                }
                crate::context_provider_config::ContextProviderDecl::Mcp { name, .. } => {
                    format!("mcp({})", name)
                }
                crate::context_provider_config::ContextProviderDecl::Workspace { name, config } => {
                    let policy = config
                        .get("policy")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(default)");
                    format!("workspace({}, policy={})", name, policy)
                }
                crate::context_provider_config::ContextProviderDecl::Knowledge { name, .. } => {
                    format!("knowledge({})", name)
                }
                crate::context_provider_config::ContextProviderDecl::Wiki { name, .. } => {
                    format!("wiki({})", name)
                }
            };

            let provider = crate::ext::build_provider_from_decl(ctx_decl);
            if provider.is_some() {
                report.resolved_providers.push(ctx_kind);
            } else {
                report.warnings.push(format!(
                    "Context provider '{}' was declared but could not be constructed. \
                     Check that dependencies are available (e.g., SKILL.md exists, MCP server is connected).",
                    ctx_kind
                ));
            }
        }

        Ok(report)
    }

    /// 加载声明并构建 Agent。
    pub async fn build(self) -> Result<Arc<dyn IAgent>> {
        let def = self.load_and_apply_overrides()?;

        match &def.kind_data {
            AgentKindData::Workflow(data) => self.build_workflow_agent(&def, data).await,
            AgentKindData::Prompt(data) => {
                if !data.sub_agents.is_empty() {
                    return self
                        .build_orchestrated(&def, data.sub_agents.clone())
                        .await;
                }
                self.build_single_prompt(&def).await
            }
            _ => Err(DeclError::Unsupported(
                "DeclAgentBuilder supports kind: prompt, kind: workflow, and orchestrated prompt agents. \
                 kind: hosted (container) agents require external deployment — parse only.".into(),
            )),
        }
    }

    /// 加载声明文件并应用运行时覆盖（model / api_key）。
    fn load_and_apply_overrides(&self) -> Result<crate::definition::AgentDefinition> {
        let mut def = if let Some(ref direct_def) = self.direct_definition {
            direct_def.clone()
        } else {
            let doc = self.load_document()?;
            doc.into_definition().ok_or_else(|| {
                DeclError::Validation(
                    "AgentDocument contains a Manifest; expected a bare AgentDefinition".into(),
                )
            })?
        };

        let prompt_data = match &mut def.kind_data {
            AgentKindData::Prompt(data) => Some(data),
            AgentKindData::Workflow(_) => None,
            _ => {
                return Err(DeclError::Unsupported(
                    "DeclAgentBuilder supports kind: prompt and kind: workflow".into(),
                ));
            }
        };

        if let Some(prompt_data) = prompt_data {
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

        Ok(def)
    }

    /// 将运行时 model / api_key 覆盖应用到 Agent 定义。
    fn apply_runtime_overrides(&self, def: &mut crate::definition::AgentDefinition) {
        let prompt_data = match &mut def.kind_data {
            AgentKindData::Prompt(data) => data,
            _ => return,
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

    /// 构建 `kind: workflow` — MAF ActionDecl 图编译为 WorkflowAgent。
    async fn build_workflow_agent(
        &self,
        def: &crate::definition::AgentDefinition,
        data: &crate::workflow_decl::WorkflowAgentData,
    ) -> Result<Arc<dyn IAgent>> {
        use rust_agent_workflow::WorkflowAgent;

        use crate::compiler::{compile_workflow, prewarm_workflow_tools};
        use crate::compiler::registry::CompileRegistry;

        let mut registry = CompileRegistry::new();
        for (name, factory) in &self.tool_factories {
            let factory = std::sync::Arc::clone(factory);
            registry.register_tool_factory(name, move |args| factory(args));
        }
        for (name, agent) in &self.workflow_agents {
            registry.register_agent(name, Arc::clone(agent));
        }

        prewarm_workflow_tools(&data.trigger.actions, &mut registry).await?;

        let graph = compile_workflow(data, &mut registry)?;
        let inner = Arc::new(WorkflowAgent::new(graph));
        Ok(crate::orchestration_builder::wrap_named_agent(def, inner, vec![]))
    }

    /// 构建带 subAgents 的多智能体编排（全部内置模式 + pipeline 闭环）。
    async fn build_orchestrated(
        &self,
        def: &crate::definition::AgentDefinition,
        sub_agent_decls: Vec<crate::definition::AgentDefinition>,
    ) -> Result<Arc<dyn IAgent>> {
        use std::collections::HashMap;

        use crate::orchestration_builder::build_orchestration_agent;
        use crate::orchestration_decl::{parse_orchestration, OrchestrationMode};

        for sub_def in &sub_agent_decls {
            if let AgentKindData::Prompt(data) = &sub_def.kind_data {
                if !data.sub_agents.is_empty() {
                    return Err(DeclError::Validation(format!(
                        "Nested subAgents are not supported in '{}'",
                        sub_def.name
                    )));
                }
            } else {
                return Err(DeclError::Validation(format!(
                    "Sub-agent '{}' must be kind: prompt",
                    sub_def.name
                )));
            }
        }

        let orch = parse_orchestration(&def.metadata, true)?;

        let uses_root_orchestrator = match orch.mode {
            OrchestrationMode::Magentic | OrchestrationMode::Pipeline => true,
            OrchestrationMode::GroupChat if orch.coordinator.is_none() => true,
            OrchestrationMode::Handoff if orch.triage.is_none() => true,
            _ => false,
        };

        let mut orchestrator = None;
        if uses_root_orchestrator {
            let mut orchestrator_def = def.clone();
            if let AgentKindData::Prompt(data) = &mut orchestrator_def.kind_data {
                data.sub_agents.clear();
            }
            orchestrator = Some(self.build_single_prompt(&orchestrator_def).await?);
        }

        let mut sub_agents: HashMap<String, Arc<dyn IAgent>> = HashMap::new();
        let parent_prompt = match &def.kind_data {
            AgentKindData::Prompt(data) => Some(data.clone()),
            _ => None,
        };

        for sub_def in sub_agent_decls {
            let mut sub_def = sub_def;
            self.apply_runtime_overrides(&mut sub_def);
            if let Some(ref parent) = parent_prompt {
                crate::context_inheritance::inherit_parent_contexts(&mut sub_def, parent);
            }
            let sub_agent = self.build_single_prompt(&sub_def).await?;
            sub_agents.insert(sub_def.name.clone(), sub_agent);
        }

        build_orchestration_agent(def, &orch, orchestrator, sub_agents)
    }

    /// 构建单个 prompt Agent（无 subAgents 编排）。
    async fn build_single_prompt(
        &self,
        def: &crate::definition::AgentDefinition,
    ) -> Result<Arc<dyn IAgent>> {
        let prompt_data = match &def.kind_data {
            AgentKindData::Prompt(data) => data,
            _ => return Err(DeclError::Unsupported("Expected prompt agent".into())),
        };

        let max_tool_rounds = self
            .max_tool_rounds
            .unwrap_or(prompt_data.max_tool_rounds);

        let chat_client = connection_resolver::resolve_chat_client_with_registry(
            &prompt_data.model,
            if self.connections.is_empty() {
                None
            } else {
                Some(&self.connections)
            },
        )?;
        let instructions = prompt_data.instructions.clone();
        let tools_list = prompt_data.tools.clone();

        let mut tool_resolver = crate::resolver::tool_resolver::ToolResolver::new();
        if !prompt_data.sandbox.is_empty() {
            tool_resolver.set_sandbox_defaults(prompt_data.sandbox.clone());
        }
        for (name, factory) in &self.tool_factories {
            let factory = Arc::clone(factory);
            tool_resolver.register_factory(name, move |args| factory(args));
        }
        let resolved_tools = tool_resolver.resolve_all(&tools_list).await?;

        // 构建上下文提供器
        let mut all_context_providers: Vec<Arc<dyn IContextProvider>> = Vec::new();
        let remaining_tools: Vec<Arc<dyn ITool>>;
        let has_workspace = prompt_data.contexts.iter().any(|d| {
            matches!(d, crate::context_provider_config::ContextProviderDecl::Workspace { .. })
        });

        // 分类工具：IScopeTool → workspace 管理，其余 → AgentBuilder 直注
        if has_workspace {
            let (scope_tools, other_tools) = partition_scope_tools(resolved_tools);
            remaining_tools = other_tools;

            // 构建 providers，workspace 特殊处理以接收 IScopeTool
            for decl in &prompt_data.contexts {
                if matches!(decl, crate::context_provider_config::ContextProviderDecl::Workspace { .. }) {
                    if let Some(ws_provider) =
                        crate::ext::build_workspace_provider(decl, &scope_tools)
                    {
                        all_context_providers.push(ws_provider);
                    }
                } else if let Some(provider) = crate::ext::build_provider_from_decl(decl) {
                    all_context_providers.push(provider);
                }
            }
        } else {
            remaining_tools = resolved_tools;
            for decl in &prompt_data.contexts {
                if let Some(provider) = crate::ext::build_provider_from_decl(decl) {
                    all_context_providers.push(provider);
                }
            }
        }

        // 合并代码注入的 provider（with_context()）
        all_context_providers.extend(self.external_contexts.iter().map(Arc::clone));

        // 自动注入 InMemoryHistoryProvider（与 AgentBuilder::new() 保持一致）
        //     确保所有 Agent 都有默认的会话历史管理。
        let has_history = all_context_providers
            .iter()
            .any(|p| p.kind() == "history");
        if !has_history {
            use rust_agent_framework::InMemoryHistoryProvider;
            all_context_providers
                .push(Arc::new(InMemoryHistoryProvider::new()));
        }

        // 通过 AgentBuilder 统一构建
        let mut builder = AgentBuilder::new(&def.name)
            .chat_client(ChatClientWrapper(chat_client))
            .instructions(&instructions);

        if !def.description.is_empty() {
            builder = builder.with_description(&def.description);
        }

        for cp in &all_context_providers {
            builder = builder.add_context_provider_shared(Arc::clone(cp));
        }

        // 注册非 IScopeTool 工具（或无 workspace 场景的全部工具）
        for tool in remaining_tools {
            builder = builder.with_tool(ToolWrapper(tool));
        }

        builder = builder.max_tool_rounds(max_tool_rounds);

        if let Some(comp) = &prompt_data.compression {
            use crate::compression_config::{build_compression_strategy, build_token_counter};
            use rust_agent_framework::EstimateCounter;

            let counter = prompt_data
                .token_counter
                .as_ref()
                .map(build_token_counter)
                .unwrap_or_else(|| Arc::new(EstimateCounter::new()));
            builder = builder
                .with_compression_strategy(build_compression_strategy(comp))
                .with_token_counter(counter);
        }

        Ok(builder.build()?)
    }

    fn load_document(&self) -> Result<AgentDocument> {
        match &self.source {
            Some(Source::File(path)) => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                match ext {
                    #[cfg(feature = "yaml")]
                    "yaml" | "yml" => AgentDocument::from_yaml_file(&path.to_string_lossy())
                        .map_err(|e| DeclError::Resolution(format!("Failed to load '{}': {}", path.display(), e))),
                    "json" => AgentDocument::from_json_file(&path.to_string_lossy())
                        .map_err(|e| DeclError::Resolution(format!("Failed to load '{}': {}", path.display(), e))),
                    #[cfg(feature = "toml")]
                    "toml" => AgentDocument::from_toml_file(&path.to_string_lossy())
                        .map_err(|e| DeclError::Resolution(format!("Failed to load '{}': {}", path.display(), e))),
                    other => Err(DeclError::Unsupported(format!(
                        "Unknown file extension '.{}' for '{}'. Expected .yaml, .json, or .toml. \
                         Use from_yaml_str() / from_json_str() / from_toml_str() for inline strings.",
                        other,
                        path.display()
                    ))),
                }
            }
            Some(Source::Str(content)) => {
                // Try parsing as YAML first (most common), fall through to JSON/TOML
                #[cfg(feature = "yaml")]
                {
                    if let Ok(doc) = AgentDocument::from_yaml_str(content) {
                        return Ok(doc);
                    }
                }
                if let Ok(doc) = AgentDocument::from_json_str(content) {
                    return Ok(doc);
                }
                #[cfg(feature = "toml")]
                {
                    if let Ok(doc) = AgentDocument::from_toml_str(content) {
                        return Ok(doc);
                    }
                }
                Err(DeclError::Validation(
                    "Failed to parse source as YAML, JSON, or TOML. \
                     Check syntax and ensure the required feature ('yaml'/'toml') is enabled."
                        .into(),
                ))
            }
            None => Err(DeclError::Validation(
                "DeclAgentBuilder requires a source: use from_file(), from_yaml_str(), or from_definition()".into(),
            )),
        }
    }
}

impl Default for DeclAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ── 辅助函数 ──

/// 将已解析的工具分为两组：(实现了 IScopeTool 的工具, 其余工具)。
///
/// IScopeTool 的工具应通过 `WorkspaceContextProvider::add_tool_arc()` 注册，
/// 以获取 scope 注入和审批包裹。其余工具直接注册到 AgentBuilder。
fn partition_scope_tools(tools: Vec<Arc<dyn ITool>>) -> (Vec<Arc<dyn ITool>>, Vec<Arc<dyn ITool>>) {
    let mut scope_tools = Vec::new();
    let mut other_tools = Vec::new();

    for tool in tools {
        // 统一检测机制——通过 ITool::as_scope_tool() 检测，无需硬编码类型列表
        if tool.as_scope_tool().is_some() {
            scope_tools.push(tool);
        } else {
            other_tools.push(tool);
        }
    }

    (scope_tools, other_tools)
}

// ── 验证 ──

/// 配置验证报告。
///
/// Re-export 自 `rust_agent_framework`，统一 AgentBuilder 和 DeclAgentBuilder
/// 的验证报告类型，消除重复定义。
pub use rust_agent_framework::ValidationReport;

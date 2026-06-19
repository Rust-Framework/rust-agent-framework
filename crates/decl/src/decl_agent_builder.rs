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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rust_agent_core::{IAgent, IContextProvider, ITool, ScopePolicy, WorkspaceScope};
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

        for tool_decl in &prompt_data.tools {
            let kind = tool_decl.kind_str();
            match tool_resolver.resolve(tool_decl).await {
                Ok(tool) => {
                    report.resolved_tools.push(format!(
                        "{} (kind={}, description from code)",
                        tool.name(),
                        kind
                    ));
                }
                Err(e) => {
                    // Levenshtein 模糊匹配
                    let name = tool_decl.name().unwrap_or("(anonymous)");
                    let suggestion = levenshtein_suggest(name, &KNOWN_TOOL_NAMES);
                    let mut msg = format!("Tool '{}' (kind={}) failed to resolve: {}", name, kind, e);
                    if let Some(s) = suggestion {
                        msg.push_str(&format!(". Did you mean '{}'?", s));
                    }
                    report.errors.push(msg);
                }
            }
        }

        // 验证 context providers
        for ctx_decl in &prompt_data.contexts {
            let ctx_kind = match ctx_decl {
                crate::context_provider_config::ContextProviderDecl::Memory { name, .. } => {
                    format!("memory({})", name)
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

            let provider = self.build_provider_from_decl(ctx_decl);
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
        // 0. 路径选择：direct_definition > source file/str
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

        // 3. 解析 chat_client
        let chat_client = connection_resolver::resolve_chat_client(&prompt_data.model)?;
        let instructions = prompt_data.instructions.clone();
        let tools_list = prompt_data.tools.clone();

        // 4. 解析工具（提前，使 workspace 可路由 IScopeTool）
        let mut tool_resolver = crate::resolver::tool_resolver::ToolResolver::new();
        for (name, factory) in &self.tool_factories {
            let factory = Arc::clone(factory);
            tool_resolver.register_factory(name, move |args| factory(args));
        }
        let resolved_tools = tool_resolver.resolve_all(&tools_list).await?;

        // 5. 构建上下文提供器
        let mut all_context_providers: Vec<Arc<dyn IContextProvider>> = Vec::new();
        let remaining_tools: Vec<Arc<dyn ITool>>;
        let has_workspace = prompt_data.contexts.iter().any(|d| {
            matches!(d, crate::context_provider_config::ContextProviderDecl::Workspace { .. })
        });

        // 5a. 分类工具：IScopeTool → workspace 管理，其余 → AgentBuilder 直注
        if has_workspace {
            let (scope_tools, other_tools) = partition_scope_tools(resolved_tools);
            remaining_tools = other_tools;

            // 5b. 构建 providers，workspace 特殊处理以接收 IScopeTool
            for decl in &prompt_data.contexts {
                if matches!(decl, crate::context_provider_config::ContextProviderDecl::Workspace { .. }) {
                    if let Some(ws_provider) = self.build_workspace_provider(decl, &scope_tools) {
                        all_context_providers.push(ws_provider);
                    }
                } else {
                    if let Some(provider) = self.build_provider_from_decl(decl) {
                        all_context_providers.push(provider);
                    }
                }
            }
        } else {
            remaining_tools = resolved_tools;
            for decl in &prompt_data.contexts {
                if let Some(provider) = self.build_provider_from_decl(decl) {
                    all_context_providers.push(provider);
                }
            }
        }

        // 5c. 合并代码注入的 provider（with_context()）
        all_context_providers.extend(self.external_contexts.iter().map(Arc::clone));

        // 5d. 自动注入 InMemoryHistoryProvider（与 AgentBuilder::new() 保持一致）
        //     确保所有 Agent 都有默认的会话历史管理。
        let has_history = all_context_providers
            .iter()
            .any(|p| p.kind() == rust_agent_core::ContextProviderKind::History);
        if !has_history {
            use rust_agent_framework::InMemoryHistoryProvider;
            all_context_providers
                .push(Arc::new(InMemoryHistoryProvider::new()));
        }

        // 6. 通过 AgentBuilder 统一构建
        let mut builder = AgentBuilder::new(&def.name)
            .chat_client(ChatClientWrapper(chat_client))
            .instructions(&instructions);

        if !def.description.is_empty() {
            builder = builder.with_description(&def.description);
        }

        for cp in &all_context_providers {
            builder = builder.add_context_provider_shared(Arc::clone(cp));
        }

        // 7. 注册非 IScopeTool 工具（或无 workspace 场景的全部工具）
        for tool in remaining_tools {
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
            ContextProviderDecl::Skills { name: skill_name, config } => {
                let dir = config
                    .get("directory")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let dir_path = if dir.is_empty() {
                    std::path::PathBuf::from("skills").join(skill_name)
                } else {
                    std::path::PathBuf::from(dir)
                };

                match rust_agent_framework::AgentSkillsProvider::scan(&dir_path)
                {
                    Ok(provider) => {
                        if provider.skills.is_empty() {
                            tracing::warn!(
                                "No SKILL.md found in skills directory '{}' for skill '{}'",
                                dir_path.display(),
                                skill_name
                            );
                        }
                        Some(Arc::new(provider))
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to scan skills directory '{}' for skill '{}': {}",
                            dir_path.display(),
                            skill_name,
                            e
                        );
                        None
                    }
                }
            }

            // ── mcp ──
            ContextProviderDecl::Mcp { name: server_name, config } => {
                let server_url = config
                    .get("serverUrl")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let server_command = config
                    .get("command")
                    .and_then(|v| v.as_str());
                let server_args = config
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    });

                // 此处仅给出引导性日志——这是已知的硬限制。
                let _ = server_command;
                let _ = server_args;
                tracing::error!(
                    "MCP declarative provider requires async connection and cannot be constructed \
                     in build_provider_from_decl. Use DeclAgentBuilder::with_context() to inject a \
                     pre-connected McpContextProvider. \
                     Server: '{}', URL: '{}'",
                    server_name,
                    if server_url.is_empty() { "(not specified)" } else { server_url }
                );
                None
            }

            // ── workspace ──
            ContextProviderDecl::Workspace { name: ws_name, config } => {
                let root = config
                    .get("root")
                    .and_then(|v| v.as_str())
                    .unwrap_or(".");
                let policy_str = config
                    .get("policy")
                    .and_then(|v| v.as_str())
                    .unwrap_or("approve");

                let policy = match policy_str {
                    "allow_all" | "allow" | "read" => ScopePolicy::AllowAll,
                    "approve_outside" | "approve" | "ask" => ScopePolicy::ApproveOutside,
                    "deny_outside" | "deny" | "restrict" => ScopePolicy::DenyOutside,
                    other => {
                        tracing::error!(
                            "Unknown workspace policy '{}' for '{}', falling back to DenyOutside (fail closed). \
                             Valid values: read/allow/allow_all, approve/ask/approve_outside, deny/restrict/deny_outside",
                            other, ws_name
                        );
                        ScopePolicy::DenyOutside
                    }
                };

                let scope = WorkspaceScope::new(root, ws_name.as_str())
                    .with_policy(policy);

                // WorkspaceContextProvider 负责注入工作区边界指令到 system prompt。
                // 工具的工作区感知由各工具自身的 IScopeTool 实现和 path_guard 处理，
                // 无需在 provider 中重复注册。
                let provider =
                    rust_agent_framework::WorkspaceContextProvider::new(
                        Arc::new(scope),
                    );
                Some(Arc::new(provider))
            }

            // ── knowledge (RAG) ──
            ContextProviderDecl::Knowledge { name: kb_name, config } => {
                let source = config
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                tracing::error!(
                    "Knowledge (RAG) declarative provider is not yet implemented. \
                     Use DeclAgentBuilder::with_context() to inject a custom knowledge provider. \
                     Knowledge base: '{}', source: '{}'",
                    kb_name,
                    if source.is_empty() { "(not specified)" } else { source }
                );
                None
            }

            // ── wiki ──
            ContextProviderDecl::Wiki { name: wiki_name, config } => {
                let source = config
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let source_display = if source.is_empty() { "(not specified)" } else { source };
                tracing::error!(
                    "Wiki declarative provider is not yet implemented. \
                     Use DeclAgentBuilder::with_context() to inject a custom wiki provider. \
                     Wiki: '{}', source: '{}'",
                    wiki_name,
                    source_display
                );
                None
            }

            ContextProviderDecl::Memory { .. } => {
                tracing::debug!("Unknown memory provider name (expected 'skill-memory')");
                None
            }
        }
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

    #[allow(dead_code)]
    fn parse_workspace_policy(_policy_str: &str) -> ScopePolicy {
        match _policy_str {
            "allow_all" | "allow" | "read" => ScopePolicy::AllowAll,
            "approve_outside" | "approve" | "ask" => ScopePolicy::ApproveOutside,
            "deny_outside" | "deny" | "restrict" => ScopePolicy::DenyOutside,
            other => {
                tracing::error!(
                    "Unknown workspace policy '{}'. Falling back to DenyOutside (fail closed). \
                     Valid values: read/allow/allow_all, approve/ask/approve_outside, deny/restrict/deny_outside",
                    other
                );
                ScopePolicy::DenyOutside
            }
        }
    }

    /// 构建 workspace 提供器并将 IScopeTool 工具路由到 workspace.add_tool_arc()，
    /// 以完成 scope 注入 + 审批包裹两阶段处理。
    fn build_workspace_provider(
        &self,
        decl: &crate::context_provider_config::ContextProviderDecl,
        scope_tools: &[Arc<dyn ITool>],
    ) -> Option<Arc<dyn IContextProvider>> {
        let (ws_name, config) = match decl {
            crate::context_provider_config::ContextProviderDecl::Workspace { name, config } => {
                (name, config)
            }
            _ => return self.build_provider_from_decl(decl),
        };

        let root = config
            .get("root")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let policy_str = config
            .get("policy")
            .and_then(|v| v.as_str())
            .unwrap_or("approve");

        let policy = match policy_str {
            "allow_all" | "allow" | "read" => ScopePolicy::AllowAll,
            "approve_outside" | "approve" | "ask" => ScopePolicy::ApproveOutside,
            "deny_outside" | "deny" | "restrict" => ScopePolicy::DenyOutside,
                    other => {
                        tracing::error!(
                            "Unknown workspace policy '{}' for '{}', falling back to DenyOutside",
                            other, ws_name
                        );
                        ScopePolicy::DenyOutside
                    }
        };

        let scope = WorkspaceScope::new(root, ws_name.as_str()).with_policy(policy);
        let mut provider =
            rust_agent_framework::WorkspaceContextProvider::new(Arc::new(scope));

        // 路由 IScopeTool 工具：逐个通过 add_tool_arc 注入 scope + 审批包裹
        for tool in scope_tools {
            provider.add_tool_arc(Arc::clone(tool));
        }

        if !scope_tools.is_empty() {
            tracing::debug!(
                "Workspace '{}' managing {} IScopeTool(s): {}",
                ws_name,
                scope_tools.len(),
                scope_tools.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ")
            );
        }

        Some(Arc::new(provider))
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
    use rust_agent_core::AsAny;

    let mut scope_tools = Vec::new();
    let mut other_tools = Vec::new();

    for tool in tools {
        let any = tool.as_any();
        // 与 WorkspaceContextProvider::try_inject_scope() 保持同步
        if any.downcast_ref::<rust_agent_framework::tools::ReadFile>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::WriteFile>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::EditFile>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::ListFiles>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::InspectFile>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::MakeDirectory>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::RemovePath>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::MoveFile>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::FindFiles>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::SearchFile>().is_some()
            || any.downcast_ref::<rust_agent_framework::tools::RunCommand>().is_some()
        {
            scope_tools.push(tool);
        } else {
            other_tools.push(tool);
        }
    }

    (scope_tools, other_tools)
}

// ── 验证 ──

/// 配置验证报告。
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// 致命错误（Agent 无法运行）。
    pub errors: Vec<String>,
    /// 警告（可运行但可能不符合预期）。
    pub warnings: Vec<String>,
    /// 已成功解析的工具列表。
    pub resolved_tools: Vec<String>,
    /// 已成功构造的上下文提供器列表。
    pub resolved_providers: Vec<String>,
    /// 解析出的模型 ID。
    pub resolved_model: Option<String>,
}

impl ValidationReport {
    /// 配置是否有效（无致命错误）。
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// 是否有任何问题。
    pub fn has_issues(&self) -> bool {
        !self.errors.is_empty() || !self.warnings.is_empty()
    }
}

/// 已知的内置工具名称列表（用于模糊匹配提示）。
const KNOWN_TOOL_NAMES: &[&str] = &[
    "read_file",
    "write_file",
    "edit_file",
    "list_files",
    "inspect_file",
    "make_directory",
    "remove_path",
    "move_file",
    "find_files",
    "search_file",
    "run_command",
    "web_search",
    "web_fetch",
    "code_interpreter",
    "load_skill",
    "read_skill_resource",
];

/// 使用 Levenshtein 距离查找最接近的匹配。
fn levenshtein_suggest(input: &str, candidates: &[&str]) -> Option<String> {
    let input_lower = input.to_lowercase();
    let mut best: Option<(&str, usize)> = None;

    for &candidate in candidates {
        let dist = levenshtein_distance(&input_lower, candidate);
        let threshold = if candidate.len() <= 4 { 1 } else { 2 };
        if dist <= threshold {
            match best {
                Some((_, best_dist)) if dist < best_dist => {
                    best = Some((candidate, dist));
                }
                None => {
                    best = Some((candidate, dist));
                }
                _ => {}
            }
        }
    }

    best.map(|(name, _)| name.to_string())
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    let mut prev = (0..=n).collect::<Vec<_>>();
    let mut curr = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

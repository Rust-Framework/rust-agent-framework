use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{
    ChatClientBuilder, IAgent, IChatClient, ICompressionStrategy, IContextProvider,
    ITokenCounter, ITool, Result, ToolRegistry,
};

use crate::ChatClientAgent;
use crate::chat_client_decorators::FunctionInvokingChatClient;
use crate::context_providers::history_provider::InMemoryHistoryProvider;

/// Fluently construct an agent with reasonable defaults.
///
/// ## Built-in context providers
///
/// `AgentBuilder` starts with `[InMemoryHistoryProvider]` as the default
/// context provider chain, providing session history management out of the box.
///
/// ## Replacing the default history management
///
/// ```ignore
/// let agent = AgentBuilder::new("agent")
///     .chat_client(client)
///     .with_history_provider(RedisHistory::new(redis))
///     .build()?;
/// ```
///
/// ## Adding providers alongside the default
///
/// ```ignore
/// let agent = AgentBuilder::new("agent")
///     .chat_client(client)
///     .add_context_provider(SkillsProvider::new())
///     .add_context_provider(RagProvider::new())
///     .build()?;
/// // Chain: [InMemoryHistoryProvider, SkillsProvider, RagProvider]
/// ```
///
/// ## Tool loop (pipeline mode)
///
/// When tools are registered, `AgentBuilder` automatically wraps the
/// `IChatClient` in a `FunctionInvokingChatClient` decorator (MAF pipeline
/// pattern).
pub struct AgentBuilder<C> {
    agent_id: String,
    chat_client: Option<C>,
    instructions: String,
    tools: Vec<Arc<dyn ITool>>,
    context_providers: Vec<Arc<dyn IContextProvider>>,
    properties: HashMap<String, serde_json::Value>,
    description: String,
    max_tool_rounds: usize,
    compression_strategy: Option<Arc<dyn ICompressionStrategy>>,
    token_counter: Option<Arc<dyn ITokenCounter>>,
}

impl<C: IChatClient + 'static> AgentBuilder<C> {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            agent_id: id.into(),
            chat_client: None,
            instructions: String::new(),
            tools: Vec::new(),
            context_providers: vec![
                Arc::new(InMemoryHistoryProvider::new()) as Arc<dyn IContextProvider>
            ],
            properties: HashMap::new(),
            description: String::new(),
            max_tool_rounds: 10,
            compression_strategy: None,
            token_counter: None,
        }
    }

    pub fn chat_client(mut self, client: C) -> Self {
        self.chat_client = Some(client);
        self
    }

    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = text.into();
        self
    }

    pub fn with_tool(mut self, tool: impl ITool + 'static) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    pub fn with_properties(
        mut self,
        iter: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) -> Self {
        for (k, v) in iter {
            self.properties.insert(k, v);
        }
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn max_tool_rounds(mut self, rounds: usize) -> Self {
        self.max_tool_rounds = rounds;
        self
    }

    /// Set the compression strategy for context window management.
    ///
    /// When set along with a token counter and model metadata,
    /// the agent will automatically compress messages that exceed
    /// the model's context window budget.
    pub fn with_compression_strategy(mut self, strategy: Arc<dyn ICompressionStrategy>) -> Self {
        self.compression_strategy = Some(strategy);
        self
    }

    /// Set the token counter for estimating token consumption.
    ///
    /// Required for compression strategies to make informed decisions.
    /// If not set, compression will not be applied even if a strategy is configured.
    pub fn with_token_counter(mut self, counter: Arc<dyn ITokenCounter>) -> Self {
        self.token_counter = Some(counter);
        self
    }

    /// 追加一个上下文提供器到链中。
    ///
    /// 提供器按注册顺序执行。不影响内置的 `InMemoryHistoryProvider`。
    /// 压缩策略（截断/窗口等）也是 ContextProvider —— 注册在最后，
    /// 设置 `replace_messages = true` 即可。
    pub fn add_context_provider(
        mut self,
        provider: impl IContextProvider + 'static,
    ) -> Self {
        self.context_providers.push(Arc::new(provider));
        self
    }

    /// 追加一个已共享（`Arc`）的上下文提供器，便于在 agent 外部保留引用。
    pub fn add_context_provider_shared(
        mut self,
        provider: Arc<dyn IContextProvider>,
    ) -> Self {
        self.context_providers.push(provider);
        self
    }

    /// 替换内置的 `InMemoryHistoryProvider`。
    ///
    /// 在链中定位 `InMemoryHistoryProvider` 并替换为指定实现。
    /// 其他 provider 保持位置不变。
    pub fn with_history_provider(
        mut self,
        provider: impl IContextProvider + 'static,
    ) -> Self {
        let pos = self
            .context_providers
            .iter()
            .position(|p| p.name() == "InMemoryHistoryProvider");
        let arc: Arc<dyn IContextProvider> = Arc::new(provider);
        match pos {
            Some(i) => self.context_providers[i] = arc,
            None => self.context_providers.push(arc),
        }
        self
    }

    /// Build the agent using ChatClient pipeline pattern.
    ///
    /// When tools are registered, the IChatClient is wrapped in a
    /// `FunctionInvokingChatClient` decorator (MAF pipeline pattern).
    pub fn build(self) -> Result<Arc<dyn IAgent>> {
        let chat_client = self.chat_client.ok_or_else(|| {
            rust_agent_core::AgentError::ConfigError("chat_client is required".into())
        })?;

        // Build the ChatClient pipeline
        let leaf: Arc<dyn IChatClient> = Arc::new(chat_client);
        let pipeline_client = if !self.tools.is_empty() {
            let tools = self.tools.clone();
            let max_rounds = self.max_tool_rounds;
            ChatClientBuilder::new()
                .leaf(leaf)
                .use_decorator(Box::new(move |inner| {
                    Arc::new(
                        FunctionInvokingChatClient::new(inner, tools.clone())
                            .with_max_rounds(max_rounds),
                    )
                }))
                .build()?
        } else {
            leaf
        };

        let mut agent = ChatClientAgent::new(&self.agent_id, pipeline_client)
            .with_instructions(&self.instructions)
            .with_context_providers(self.context_providers);

        if let Some(strategy) = self.compression_strategy {
            agent = agent.with_compression_strategy(strategy);
        }
        if let Some(counter) = self.token_counter {
            agent = agent.with_token_counter(counter);
        }

        if !self.description.is_empty() {
            agent = agent.with_description(&self.description);
        }

        if !self.tools.is_empty() {
            let mut registry = ToolRegistry::new();
            for t in &self.tools {
                registry.register_arc(Arc::clone(t));
            }
            agent = agent.with_tools(registry);
        }

        Ok(Arc::new(agent))
    }
}

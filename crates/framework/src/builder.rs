use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{
    ChatClientBuilder, IAgent, IChatClient, ICompressionStrategy, IContextProvider,
    ITokenCounter, ITool, Result, ToolRegistry,
};

use crate::ChatClientAgent;
use crate::chat_client_decorators::FunctionInvokingChatClient;
use crate::context_providers::history_provider::InMemoryHistoryProvider;

/// 以流畅的构建器模式创建 Agent，提供合理的默认值。
///
/// ## 内置上下文提供器
///
/// `AgentBuilder` 默认包含 `[InMemoryHistoryProvider]` 作为
/// 上下文提供器链，提供开箱即用的会话历史管理。
///
/// ## 替换默认历史管理
///
/// ```ignore
/// let agent = AgentBuilder::new("agent")
///     .chat_client(client)
///     .with_history_provider(RedisHistory::new(redis))
///     .build()?;
/// ```
///
/// ## 在默认基础上添加提供器
///
/// ```ignore
/// let agent = AgentBuilder::new("agent")
///     .chat_client(client)
///     .add_context_provider(SkillsProvider::new())
///     .add_context_provider(RagProvider::new())
///     .build()?;
/// // 链式顺序：[InMemoryHistoryProvider, SkillsProvider, RagProvider]
/// ```
///
/// ## 工具循环（管道模式）
///
/// 注册工具时，`AgentBuilder` 自动将 `IChatClient` 包裹在
/// `FunctionInvokingChatClient` 装饰器中（MAF 管道模式）。
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
    /// 创建新的 AgentBuilder 实例
    ///
    /// 初始化默认上下文提供器链，包含 `InMemoryHistoryProvider`。
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

    /// 设置聊天客户端
    pub fn chat_client(mut self, client: C) -> Self {
        self.chat_client = Some(client);
        self
    }

    /// 设置系统指令文本
    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = text.into();
        self
    }

    /// 注册一个工具
    pub fn with_tool(mut self, tool: impl ITool + 'static) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    /// 设置 Agent 属性键值对
    pub fn with_properties(
        mut self,
        iter: impl IntoIterator<Item = (String, serde_json::Value)>,
    ) -> Self {
        for (k, v) in iter {
            self.properties.insert(k, v);
        }
        self
    }

    /// 设置 Agent 描述信息
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    /// 设置最大工具调用轮数
    pub fn max_tool_rounds(mut self, rounds: usize) -> Self {
        self.max_tool_rounds = rounds;
        self
    }

    /// 设置上下文窗口压缩策略
    ///
    /// 配合 token 计数器和模型元数据使用时，Agent 会在消息超出
    /// 模型上下文窗口预算时自动进行压缩。
    pub fn with_compression_strategy(mut self, strategy: Arc<dyn ICompressionStrategy>) -> Self {
        self.compression_strategy = Some(strategy);
        self
    }

    /// 设置 Token 计数器
    ///
    /// 压缩策略需要 Token 计数器来做出合理的压缩决策。
    /// 如果未设置，即使配置了压缩策略也不会生效。
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

    /// 构建 Agent，采用 ChatClient 管道模式
    ///
    /// 注册工具时，IChatClient 会被包裹在 `FunctionInvokingChatClient`
    /// 装饰器中（MAF 管道模式）。
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

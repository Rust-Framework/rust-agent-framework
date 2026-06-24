use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentMetadata, IChatClient, ICompressionStrategy, IContextProvider, ITokenCounter,
    ToolRegistry,
};

/// ChatClientAgent — IAgent 实现，对齐 MAF ChatClientAgent。
///
/// 持有 instructions、tools 和 context_providers 链。
/// `InMemoryHistoryProvider` 由 AgentBuilder 默认注入。
/// Provider 链按注册顺序执行，靠后的 Provider 可设置
/// `ContextResult.replace_messages = true` 来实现压缩/截断。
pub struct ChatClientAgent {
    pub(super) id: AgentId,
    pub(super) metadata: AgentMetadata,
    pub(super) chat_client: Arc<dyn IChatClient>,
    pub(super) instructions: String,
    pub(super) tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    pub(super) context_providers: Vec<Arc<dyn IContextProvider>>,
    pub(super) compression_strategy: Option<Arc<dyn ICompressionStrategy>>,
    pub(super) token_counter: Option<Arc<dyn ITokenCounter>>,
}

impl ChatClientAgent {
    pub fn new(name: impl Into<String>, chat_client: Arc<dyn IChatClient>) -> Self {
        let name = name.into();
        Self {
            id: AgentId::new(&name),
            metadata: AgentMetadata {
                agent_type: "ChatClientAgent".to_string(),
                key: name.clone(),
                description: String::new(),
                ..Default::default()
            },
            chat_client,
            instructions: String::new(),
            tools: Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
            context_providers: Vec::new(),
            compression_strategy: None,
            token_counter: None,
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Arc::new(tokio::sync::RwLock::new(tools));
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = description.into();
        self
    }

    pub fn with_context_providers(
        mut self,
        providers: Vec<Arc<dyn IContextProvider>>,
    ) -> Self {
        self.context_providers = providers;
        self
    }

    pub fn with_compression_strategy(
        mut self,
        strategy: Arc<dyn ICompressionStrategy>,
    ) -> Self {
        self.compression_strategy = Some(strategy);
        self
    }

    pub fn with_token_counter(mut self, counter: Arc<dyn ITokenCounter>) -> Self {
        self.token_counter = Some(counter);
        self
    }

    pub async fn tools(&self) -> tokio::sync::RwLockReadGuard<'_, ToolRegistry> {
        self.tools.read().await
    }
}

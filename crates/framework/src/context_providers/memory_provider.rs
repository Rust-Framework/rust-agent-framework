use async_trait::async_trait;
use std::sync::Arc;

use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection, IAgent, IContextProvider,
    ISession, IVectorStore, Result,
};

/// 记忆上下文提供者，将聊天消息存入向量数据库
///
/// 参照 MAF 的 ChatHistoryMemoryProvider 设计。
/// 提供两种模式：
/// - `AutoInject`：每次调用前自动检索相关记忆并注入
/// - `OnDemand`：暴露搜索工具，由模型按需调用
pub struct MemoryContextProvider {
    vector_store: Arc<dyn IVectorStore>,
    mode: MemoryMode,
}

/// 记忆检索模式
pub enum MemoryMode {
    /// 每次调用前自动检索 top_k 条相关记忆并注入到上下文
    AutoInject { top_k: usize },
    /// 暴露搜索工具，由模型按需调用（暂未实现）
    OnDemand,
}

impl MemoryContextProvider {
    pub fn new(vector_store: Arc<dyn IVectorStore>, mode: MemoryMode) -> Self {
        Self { vector_store, mode }
    }

    pub fn with_auto_inject(vector_store: Arc<dyn IVectorStore>, top_k: usize) -> Self {
        Self::new(vector_store, MemoryMode::AutoInject { top_k })
    }

    pub fn with_on_demand(vector_store: Arc<dyn IVectorStore>) -> Self {
        Self::new(vector_store, MemoryMode::OnDemand)
    }
}

#[async_trait]
impl IContextProvider for MemoryContextProvider {
    fn name(&self) -> &str {
        "MemoryContextProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextInjection> {
        // TODO: Implement memory retrieval
        // 1. Generate embedding for the last user message
        // 2. Search vector store for relevant memories
        // 3. Inject as context messages
        Ok(ContextInjection::default())
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        // TODO: Implement memory storage
        // 1. Extract key information from the conversation
        // 2. Generate embedding
        // 3. Upsert to vector store
        Ok(())
    }
}

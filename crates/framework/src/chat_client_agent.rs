use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::RwLock;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentStreamChunk, BoxStream, ChatMessage,
    IAgent, IChatClient, IMiddleware, MessageRole, Result, ToolRegistry,
};

/// ChatClientAgent — the primary IAgent implementation following MAF.
///
/// Composes a chat client with instructions, tools, middleware,
/// and session management. Only streaming output is supported.
pub struct ChatClientAgent {
    id: AgentId,
    metadata: AgentMetadata,
    chat_client: Arc<dyn IChatClient>,
    instructions: String,
    tools: Arc<RwLock<ToolRegistry>>,
    middleware: Vec<Arc<dyn IMiddleware>>,
    history: Arc<RwLock<Vec<ChatMessage>>>,
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
            },
            chat_client,
            instructions: String::new(),
            tools: Arc::new(RwLock::new(ToolRegistry::new())),
            middleware: Vec::new(),
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Arc::new(RwLock::new(tools));
        self
    }

    pub fn with_middleware(mut self, middleware: Arc<dyn IMiddleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.metadata.description = description.into();
        self
    }

    pub async fn tools(&self) -> tokio::sync::RwLockReadGuard<'_, ToolRegistry> {
        self.tools.read().await
    }

    pub async fn tools_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, ToolRegistry> {
        self.tools.write().await
    }
}

#[async_trait]
impl IAgent for ChatClientAgent {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }

    async fn run(&self, messages: Vec<ChatMessage>) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        // Apply request middleware
        let mut processed_messages = messages;
        for mw in &self.middleware {
            mw.on_request(&mut processed_messages).await?;
        }

        // Build full message list: instructions + history + new messages
        let mut full_messages = Vec::new();
        if !self.instructions.is_empty() {
            full_messages.push(ChatMessage::system(&self.instructions));
        }
        {
            let history = self.history.read().await;
            full_messages.extend(history.iter().cloned());
        }

        // Store new user messages in history (before moving processed_messages)
        {
            let mut history = self.history.write().await;
            for msg in &processed_messages {
                if matches!(msg.role, MessageRole::User | MessageRole::Tool) {
                    history.push(msg.clone());
                }
            }
        }

        full_messages.extend(processed_messages);

        // Stream from chat client, mapping ChatStreamChunk -> AgentStreamChunk
        let chat_stream = self.chat_client.run(&full_messages).await?;
        let agent_id = self.id.clone();

        let mapped = chat_stream.map(move |chunk_result| {
            chunk_result.map(|chunk| AgentStreamChunk {
                text_delta: chunk.text_delta,
                tool_call_delta: chunk.tool_call_delta,
                source_agent_id: Some(agent_id.clone()),
            })
        });

        Ok(Box::pin(mapped))
    }

    async fn reset(&self) -> Result<()> {
        self.history.write().await.clear();
        Ok(())
    }
}

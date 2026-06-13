use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::RwLock;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentStreamChunk, BoxStream, ChatAgentRunOptions,
    ChatMessage, IAgent, IChatClient, IMiddleware, MessageRole, Result, ToolRegistry,
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

    /// Append a message to the agent's internal conversation history.
    pub async fn add_message(&self, message: ChatMessage) {
        self.history.write().await.push(message);
    }

    /// Clear the agent's internal conversation history.
    pub async fn clear_history(&self) {
        self.history.write().await.clear();
    }
}

#[async_trait]
impl IAgent for ChatClientAgent {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        options: ChatAgentRunOptions,
    ) -> Result<BoxStream<Result<AgentStreamChunk>>> {
        // Apply request middleware
        let mut processed_messages = messages;
        for mw in &self.middleware {
            mw.on_request(&mut processed_messages).await?;
        }

        // Build full message list: instructions + history + new messages
        // Per-call instructions override the agent's default
        let effective_instructions = options.instructions.as_deref().unwrap_or(&self.instructions);
        let mut full_messages = Vec::new();
        if !effective_instructions.is_empty() {
            full_messages.push(ChatMessage::system(effective_instructions));
        }
        {
            let history = self.history.read().await;
            full_messages.extend(history.iter().cloned());
        }

        // Store new user messages in history
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
        let mut client_run_options = options.to_chat_client_run_options();

        // Serialize registered tools into OpenAI function-calling format
        {
            let registry = self.tools.read().await;
            if !registry.is_empty() {
                let tool_defs: Vec<serde_json::Value> = registry
                    .list()
                    .iter()
                    .map(|tool| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": tool.name(),
                                "description": tool.description(),
                                "parameters": tool.parameters_schema(),
                            }
                        })
                    })
                    .collect();
                client_run_options.tools = tool_defs;
            }
        }

        let chat_stream = self.chat_client.run(&full_messages, client_run_options).await?;
        let agent_id = self.id.clone();

        // Shared buffer to accumulate assistant text across stream chunks
        let assistant_buf = Arc::new(tokio::sync::RwLock::new(String::new()));
        let buf_clone = assistant_buf.clone();

        let mapped = chat_stream.map(move |chunk_result| {
            let chunk = match chunk_result {
                Ok(c) => c,
                Err(e) => return Err(e),
            };
            // Accumulate assistant text for history (non-blocking best-effort)
            if let Some(ref delta) = chunk.text_delta {
                // We can't await inside map, so store via try_write
                if let Ok(mut buf) = buf_clone.try_write() {
                    buf.push_str(delta);
                }
            }
            Ok(AgentStreamChunk {
                text_delta: chunk.text_delta,
                tool_call_delta: chunk.tool_call_delta,
                reasoning_delta: chunk.reasoning_delta,
                source_agent_id: Some(agent_id.clone()),
            })
        });

        // After stream ends, store assistant message in history
        let history = self.history.clone();
        let final_stream = mapped.chain(futures_util::stream::once(async move {
            let text = assistant_buf.read().await.clone();
            if !text.is_empty() {
                history.write().await.push(ChatMessage::assistant(&text));
            }
            Ok(AgentStreamChunk {
                text_delta: None,
                tool_call_delta: None,
                reasoning_delta: None,
                source_agent_id: None,
            })
        }));

        Ok(Box::pin(final_stream))
    }

    async fn reset(&self) -> Result<()> {
        self.history.write().await.clear();
        Ok(())
    }
}

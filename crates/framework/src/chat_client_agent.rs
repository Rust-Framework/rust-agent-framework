use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentResponseUpdate, AgentRunOptions, BoxStream,
    ChatMessage, FinishReason, IAgent, IChatClient, ISession, MessageRole, Result, ToolRegistry,
    Usage,
};
use crate::converter::AgentResponseConverter;

/// ChatClientAgent — the primary IAgent implementation following MAF.
///
/// Composes a chat client with instructions, tools, and session management.
/// Only streaming output is supported.
pub struct ChatClientAgent {
    id: AgentId,
    metadata: AgentMetadata,
    chat_client: Arc<dyn IChatClient>,
    instructions: String,
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
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
            tools: Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())),
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

    pub async fn tools(&self) -> tokio::sync::RwLockReadGuard<'_, ToolRegistry> {
        self.tools.read().await
    }

    pub async fn tools_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, ToolRegistry> {
        self.tools.write().await
    }
}

#[async_trait]
impl IAgent for ChatClientAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let run_options = options.unwrap_or_default();

        // 1. Determine effective instructions (per-call overrides agent default)
        let effective_instructions = run_options
            .instructions
            .clone()
            .unwrap_or_else(|| self.instructions.clone());

        // 2. Build full messages: [system] + [session_history] + [user/tool messages]
        let mut full_messages = Vec::new();

        if !effective_instructions.is_empty() {
            full_messages.push(ChatMessage::system(&effective_instructions));
        }

        if let Some(ref sess) = session {
            if let Ok(history) = sess.get_messages().await {
                full_messages.extend(history);
            }
        }

        for msg in &messages {
            if msg.role == MessageRole::User || msg.role == MessageRole::Tool {
                full_messages.push(msg.clone());
            }
        }

        // 3. Build ChatClientRunOptions from AgentRunOptions
        let mut client_opts = run_options.to_chat_client_run_options();

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
                client_opts.tools = tool_defs;
            }
        }

        // 4. Call chat client — get raw AgentResponseUpdate stream
        let stream = self.chat_client.run(&full_messages, client_opts).await?;

        // 5. Convert AgentResponseUpdate stream → AgentResponseResult stream
        let agent_id = self.id.clone();
        let executor_id = self.id.to_string();
        let converter = AgentResponseConverter::new(agent_id, executor_id, &run_options);

        let converted = futures_util::stream::unfold(
            // State: (stream, converter, pending_finish, pending_usage)
            (stream, converter, None::<FinishReason>, None::<Usage>),
            |(mut stream, mut converter, mut pending_finish, mut pending_usage)| async move {
                loop {
                    match stream.next().await {
                        Some(Ok(update)) => {
                            // Track finish_reason and usage before consuming
                            if let AgentResponseUpdate::Finish {
                                ref finish_reason,
                                ref usage,
                            } = update
                            {
                                pending_finish = Some(finish_reason.clone());
                                if usage.is_some() {
                                    pending_usage = usage.clone();
                                }
                            }

                            let output = converter.consume(update);
                            if !output.contents.is_empty() || !output.events.is_empty() {
                                return Some((
                                    Ok(AgentResponseResult {
                                        id: None,
                                        model: None,
                                        finish_reason: None,
                                        contents: output.contents,
                                        events: output.events,
                                    }),
                                    (stream, converter, pending_finish, pending_usage),
                                ));
                            }
                            // Empty output — continue polling
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(e),
                                (stream, converter, pending_finish, pending_usage),
                            ));
                        }
                        None => {
                            // Stream ended — emit finalize result
                            let final_result =
                                converter.finalize(pending_finish.clone(), pending_usage.clone());
                            if !final_result.contents.is_empty()
                                || final_result.finish_reason.is_some()
                                || !final_result.events.is_empty()
                            {
                                return Some((
                                    Ok(final_result),
                                    (stream, converter, pending_finish, pending_usage),
                                ));
                            }
                            return None;
                        }
                    }
                }
            },
        );

        Ok(Box::pin(converted))
    }

    fn get_subagent(&self, _agent_id: &AgentId) -> Option<Arc<dyn IAgent>> {
        None
    }

    fn list_subagents(&self) -> Vec<Arc<dyn IAgent>> {
        vec![]
    }

    async fn reset(&self) -> Result<()> {
        Ok(())
    }
}

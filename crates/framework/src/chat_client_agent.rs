use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use tracing;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponse, AgentResponseResult, AgentResponseUpdate,
    AgentRunOptions, BoxStream, ChatMessage, Content, FinishReason, IAgent, IChatClient,
    ICompressionStrategy, IContextProvider, ISession, ITokenCounter, MessageRole, Result,
    ToolCall, ToolRegistry, Usage,
};
use crate::converter::AgentResponseConverter;
use crate::memory::memory_context::build_turn_transcript;

/// ChatClientAgent — IAgent 实现，对齐 MAF ChatClientAgent。
///
/// 持有 instructions、tools 和 context_providers 链。
/// `InMemoryHistoryProvider` 由 AgentBuilder 默认注入。
/// Provider 链按注册顺序执行，靠后的 Provider 可设置
/// `ContextInjection.replace_messages = true` 来实现压缩/截断。
pub struct ChatClientAgent {
    id: AgentId,
    metadata: AgentMetadata,
    chat_client: Arc<dyn IChatClient>,
    instructions: String,
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    context_providers: Vec<Arc<dyn IContextProvider>>,
    compression_strategy: Option<Arc<dyn ICompressionStrategy>>,
    token_counter: Option<Arc<dyn ITokenCounter>>,
}

struct AgentProxy {
    id: AgentId,
    metadata: AgentMetadata,
    chat_client: Arc<dyn IChatClient>,
}

#[async_trait]
impl IAgent for AgentProxy {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }
    fn chat_client(&self) -> Option<&Arc<dyn IChatClient>> { Some(&self.chat_client) }
    async fn run(&self, _: Vec<ChatMessage>, _: Option<Arc<dyn ISession>>, _: Option<AgentRunOptions>) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        Err(rust_agent_core::AgentError::ConfigError("AgentProxy::run not supported".into()))
    }
    async fn reset(&self) -> Result<()> { Ok(()) }
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

#[async_trait]
impl IAgent for ChatClientAgent {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let run_options = options.unwrap_or_default();

        // Caller messages only (before provider/history merge) — used for turn transcript.
        let caller_messages: Vec<ChatMessage> = messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .cloned()
            .collect();

        // ── Phase 1: Pre-invocation ───────────────────────────────────
        let mut merged_instructions = String::new();
        let mut merged_provider_messages = Vec::new();
        let mut merged_provider_tools = Vec::new();

        if let Some(ref sess) = session {
            for provider in &self.context_providers {
                let injection = provider
                    .on_invoking(self, sess.as_ref(), &messages, &run_options)
                    .await
                    .unwrap_or_default();

                if let Some(inst) = injection.instructions {
                    if !merged_instructions.is_empty() {
                        merged_instructions.push_str("\n\n");
                    }
                    merged_instructions.push_str(&inst);
                }

                if injection.replace_messages {
                    // 替换模式：压缩策略等用此清空前面累积的消息
                    merged_provider_messages = injection.messages;
                } else {
                    merged_provider_messages.extend(injection.messages);
                }
                merged_provider_tools.extend(injection.tools);
            }
        }

        // 组装 [system] + [provider_messages] + [caller_messages]
        let effective_instructions = run_options
            .instructions
            .clone()
            .unwrap_or_else(|| self.instructions.clone());

        let mut full_messages = Vec::new();
        let mut sys = effective_instructions;
        if !merged_instructions.is_empty() {
            if !sys.is_empty() { sys.push_str("\n\n"); }
            sys.push_str(&merged_instructions);
        }
        if !sys.is_empty() {
            full_messages.push(ChatMessage::system(&sys));
        }
        full_messages.extend(
            merged_provider_messages
                .into_iter()
                .filter(|m| m.role != MessageRole::System),
        );
        full_messages.extend(
            messages
                .into_iter()
                .filter(|m| m.role != MessageRole::System),
        );

        let original_request_messages = full_messages.clone();

        // ── Phase 1.5: Compression ────────────────────────────────────
        if let (Some(ref strategy), Some(ref counter)) = (&self.compression_strategy, &self.token_counter) {
            if let Some(model_metadata) = self.chat_client.model_metadata() {
                let budget = model_metadata.input_budget();
                let current_tokens = counter.count_tokens(&full_messages);
                if current_tokens > budget {
                    tracing::info!(
                        strategy = strategy.name(),
                        current_tokens = current_tokens,
                        budget = budget,
                        "Applying compression — token budget exceeded"
                    );
                    match strategy.compress(full_messages.clone(), budget, counter.as_ref()) {
                        Ok(compressed) => {
                            let new_tokens = counter.count_tokens(&compressed);
                            tracing::info!(
                                strategy = strategy.name(),
                                before_tokens = current_tokens,
                                after_tokens = new_tokens,
                                "Compression completed"
                            );
                            full_messages = compressed;
                        }
                        Err(e) => {
                            tracing::warn!(
                                strategy = strategy.name(),
                                error = %e,
                                "Compression failed, using original messages"
                            );
                        }
                    }
                }
            }
        }

        // KV cache 追踪
        if let Some(ref sess) = session {
            sess.touch_request_hash(&full_messages);
        }

        // ── Phase 2: LLM 调用 ────────────────────────────────────────
        let mut client_opts = run_options.to_chat_client_run_options();
        {
            let registry = self.tools.read().await;
            let mut tool_defs: Vec<serde_json::Value> = if !registry.is_empty() {
                registry.list().iter().map(|tool| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": tool.name(),
                            "description": tool.description(),
                            "parameters": tool.parameters(),
                        }
                    })
                }).collect()
            } else {
                Vec::new()
            };

            // 按名称去重：若 provider 注入的工具与 registry 或已有 provider 工具同名，则跳过
            use std::collections::HashSet;
            let mut seen: HashSet<String> = tool_defs
                .iter()
                .filter_map(|d| d["function"]["name"].as_str().map(String::from))
                .collect();

            for tool in &merged_provider_tools {
                let name = tool.name().to_string();
                if seen.insert(name) {
                    tool_defs.push(serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": tool.name(),
                            "description": tool.description(),
                            "parameters": tool.parameters(),
                        }
                    }));
                }
            }
            if !tool_defs.is_empty() {
                client_opts.tools = tool_defs;
            }
        }

        // Carry provider-injected tools through ChatClientRunOptions so
        // FunctionInvokingChatClient can execute them — not just send
        // their schemas to the LLM.  Follows MAF's ChatOptions.Tools pattern.
        client_opts.provider_tools = merged_provider_tools;

        let stream = self.chat_client.run(&full_messages, client_opts).await?;
        let agent_id = self.id.clone();
        let executor_id = self.id.to_string();
        let converter = AgentResponseConverter::new(agent_id, executor_id, &run_options);

        let converted = futures_util::stream::unfold(
            (stream, converter, None::<FinishReason>, None::<Usage>, false),
            |(mut stream, mut converter, mut pf, mut pu, done)| async move {
                if done { return None; }
                loop {
                    match stream.next().await {
                        Some(Ok(update)) => {
                            if let AgentResponseUpdate::Finish { ref finish_reason, ref usage } = update {
                                pf = Some(finish_reason.clone());
                                if usage.is_some() { pu = usage.clone(); }
                            }
                            let output = converter.consume(update);
                            if !output.contents.is_empty() || !output.events.is_empty() {
                                return Some((Ok(AgentResponseResult {
                                    id: None, model: None, finish_reason: None,
                                    contents: output.contents, events: output.events,
                                }), (stream, converter, pf, pu, false)));
                            }
                        }
                        Some(Err(e)) => return Some((Err(e), (stream, converter, pf, pu, false))),
                        None => {
                            let fr = converter.finalize(pf.clone(), pu.clone());
                            return Some((Ok(fr), (stream, converter, pf, pu, true)));
                        }
                    }
                }
            },
        );

        // ── Phase 3: Channel 分叉 — 非阻塞 post-invocation ───────────
        if !self.context_providers.is_empty() {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let providers = Arc::new(self.context_providers.clone());
            let session_for_invoked = session.clone();
            let request_messages = original_request_messages;
            let caller_messages_for_transcript = caller_messages;
            let agent_id_proxy = self.id.clone();
            let agent_meta_proxy = self.metadata.clone();
            let chat_client_proxy = self.chat_client.clone();

            tokio::spawn(async move {
                let mut collected: Vec<Result<AgentResponseResult>> = Vec::new();
                while let Some(chunk) = rx.recv().await { collected.push(chunk); }
                if collected.is_empty() { return; }

                let mut text = String::new();
                let mut tool_calls = Vec::new();
                let mut tool_results: Vec<(String, Option<String>, Option<String>)> = Vec::new();
                let mut source_agent_id = None;
                let mut finish_reason = None;
                let flat_chunks: Vec<AgentResponseResult> =
                    collected.iter().flatten().cloned().collect();
                for chunk in collected.iter().flatten() {
                    if chunk.finish_reason.is_some() { finish_reason = chunk.finish_reason.clone(); }
                    for content in &chunk.contents {
                        if let Content::Text(c) = content { text.push_str(&c.delta); }
                        if let Content::ToolCalling(c) = content {
                            // Normalize arguments to Value::String for consistent
                            // downstream handling. flush_tool_calls() may have
                            // parsed them into Value::Object via serde_json::from_str.
                            let args = match &c.arguments {
                                serde_json::Value::String(_) => c.arguments.clone(),
                                other => serde_json::Value::String(other.to_string()),
                            };
                            tool_calls.push(ToolCall {
                                id: c.call_id.clone(), name: c.name.clone(), arguments: args,
                            });
                            if source_agent_id.is_none() { source_agent_id = c.meta.agent_id.clone(); }
                        }
                        if let Content::ToolCalled(c) = content {
                            tool_results.push((c.call_id.clone(), c.result.clone(), c.error.clone()));
                        }
                    }
                }
                let mut tool_result_messages = Vec::new();
                for tc in &tool_calls {
                    let content = tool_results
                        .iter()
                        .find(|(id, _, _)| id == &tc.id)
                        .and_then(|(_, result, error)| error.clone().or_else(|| result.clone()))
                        .unwrap_or_default();
                    tool_result_messages.push(ChatMessage::tool(content, &tc.id));
                }
                let turn_transcript =
                    build_turn_transcript(&caller_messages_for_transcript, &flat_chunks);
                let response = AgentResponse {
                    id: None, model: None, text, reasoning_text: None, tool_calls,
                    tool_messages: tool_result_messages,
                    turn_transcript,
                    finish_reason, usage: None, source_agent_id,
                };
                let proxy = AgentProxy { id: agent_id_proxy, metadata: agent_meta_proxy, chat_client: chat_client_proxy };

                if let Some(ref sess) = session_for_invoked {
                    for provider in providers.iter() {
                        if let Err(e) = provider.on_invoked(
                            &proxy, sess.as_ref(), &request_messages, Some(&response), None,
                        ).await {
                            tracing::warn!(provider = %provider.name(), error = %e, "on_invoked failed");
                        }
                    }

                    // Persist assistant + tool messages from full turn transcript.
                    if !response.turn_transcript.is_empty() {
                        let non_user: Vec<ChatMessage> = response
                            .turn_transcript
                            .iter()
                            .filter(|m| m.role != MessageRole::User)
                            .cloned()
                            .collect();
                        if !non_user.is_empty() {
                            if let Err(e) = sess.add_messages_batch(&non_user).await {
                                tracing::warn!(error = %e, "Failed to persist turn transcript to session");
                            }
                        }
                    } else if !response.tool_calls.is_empty() {
                        if let Err(e) = sess.add_message(ChatMessage::assistant_with_tools(
                            response.text.clone(),
                            response.tool_calls.clone(),
                        )).await {
                            tracing::warn!(error = %e, "Failed to persist assistant+tool_calls message to session");
                        }
                        for tm in &response.tool_messages {
                            if let Err(e) = sess.add_message(tm.clone()).await {
                                tracing::warn!(error = %e, "Failed to persist tool result message to session");
                            }
                        }
                    } else if !response.text.is_empty() {
                        if let Err(e) = sess.add_message(ChatMessage::assistant(response.text.clone())).await {
                            tracing::warn!(error = %e, "Failed to persist assistant message to session");
                        }
                    }
                }
            });

            let stream = converted.inspect(move |chunk| {
                if let Ok(ref c) = chunk { let _ = tx.send(Ok(c.clone())); }
            });
            return Ok(Box::pin(stream));
        }

        Ok(Box::pin(converted))
    }

    async fn reset(&self) -> Result<()> { Ok(()) }

    /// Expose the underlying chat client so context providers (e.g.
    /// `SkillMemoryContextProvider`) can auto-discover it to spawn
    /// sub-agents like `MemoryAgent`.
    fn chat_client(&self) -> Option<&Arc<dyn IChatClient>> {
        Some(&self.chat_client)
    }
}

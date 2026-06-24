use async_trait::async_trait;
use futures_util::StreamExt;
use std::collections::HashSet;
use std::sync::Arc;

use rust_agent_core::{
    AgentResponseResult, AgentResponseUpdate, AgentRunOptions, BoxStream, ChatMessage,
    FinishReason, IAgent, IChatClient, ISession, MessageRole, Result, Usage,
};

use crate::converter::AgentResponseConverter;

use super::chat_client::ChatClientAgent;

#[async_trait]
impl IAgent for ChatClientAgent {
    fn id(&self) -> &rust_agent_core::AgentId {
        &self.id
    }

    fn metadata(&self) -> &rust_agent_core::AgentMetadata {
        &self.metadata
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let run_options = options.unwrap_or_default();

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
            let ctx = rust_agent_core::ProviderContext {
                agent: self,
                session: sess.as_ref(),
                messages: &messages,
                options: &run_options,
            };
            for provider in &self.context_providers {
                if let Some(inst) = provider.enrich_instructions(&ctx).await.unwrap_or(None) {
                    if !merged_instructions.is_empty() {
                        merged_instructions.push_str("\n\n");
                    }
                    merged_instructions.push_str(&inst);
                }

                let injection = provider.enrich_messages(&ctx).await.unwrap_or_default();
                if injection.replace {
                    merged_provider_messages = injection.messages;
                } else {
                    merged_provider_messages.extend(injection.messages);
                }
                merged_provider_tools.extend(provider.enrich_tools(&ctx).await.unwrap_or_default());
            }
        }

        let effective_instructions = run_options
            .instructions
            .clone()
            .unwrap_or_else(|| self.instructions.clone());

        let mut full_messages = Vec::new();
        let mut sys = effective_instructions;
        if !merged_instructions.is_empty() {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
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
        if let (Some(ref strategy), Some(ref counter)) =
            (&self.compression_strategy, &self.token_counter)
        {
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

        if let Some(ref sess) = session {
            sess.touch_request_hash(&full_messages);
        }

        // ── Phase 2: LLM 调用 ────────────────────────────────────────
        let mut client_opts = run_options.to_chat_client_run_options();
        {
            let registry = self.tools.read().await;
            let mut tool_defs: Vec<serde_json::Value> = if !registry.is_empty() {
                registry
                    .list()
                    .iter()
                    .map(|tool| {
                        serde_json::json!({
                            "type": "function",
                            "function": {
                                "name": tool.name(),
                                "description": tool.description(),
                                "parameters": tool.parameters(),
                            }
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };

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

        client_opts.provider_tools = merged_provider_tools;

        let stream = self.chat_client.run(&full_messages, client_opts).await?;
        let agent_id = self.id.clone();
        let executor_id = self.id.to_string();
        let converter = AgentResponseConverter::new(agent_id, executor_id, &run_options);

        let converted = futures_util::stream::unfold(
            (stream, converter, None::<FinishReason>, None::<Usage>, false),
            |(mut stream, mut converter, mut pf, mut pu, done)| async move {
                if done {
                    return None;
                }
                loop {
                    match stream.next().await {
                        Some(Ok(update)) => {
                            if let AgentResponseUpdate::Finish {
                                ref finish_reason,
                                ref usage,
                            } = update
                            {
                                pf = Some(finish_reason.clone());
                                if usage.is_some() {
                                    pu = usage.clone();
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
                                    (stream, converter, pf, pu, false),
                                ));
                            }
                        }
                        Some(Err(e)) => {
                            return Some((Err(e), (stream, converter, pf, pu, false)));
                        }
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
            let stream = self.spawn_post_invocation_handler(
                converted,
                session,
                original_request_messages,
                caller_messages,
            );
            return Ok(Box::pin(stream));
        }

        Ok(Box::pin(converted))
    }

    async fn reset(&self) -> Result<()> {
        Ok(())
    }

    fn chat_client(&self) -> Option<&Arc<dyn IChatClient>> {
        Some(&self.chat_client)
    }
}

use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, IAgent, IChatClient, ITool, MessageRole,
    ToolRegistry,
};

use crate::tools::{ReadFile, WriteFile};
use crate::chat_client_decorators::FunctionInvokingChatClient;
use crate::ChatClientAgent;

use super::index_audit::{format_index_gaps, scan_index_gaps};
use super::memory_context::build_consolidation_context;

/// Run MemoryAgent for memory consolidation.
///
/// `consolidation_messages` is the selective projection of factual conversation
/// context (no MainAgent system).  `AGENT.md` is injected separately as system.
pub(crate) async fn run_memory_agent(
    memory_dir: PathBuf,
    client: Arc<dyn IChatClient>,
    consolidation_messages: Vec<ChatMessage>,
) {
    let agent_md_path = memory_dir.join("AGENT.md");
    let agent_md = match std::fs::read_to_string(&agent_md_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, path = %agent_md_path.display(), "Failed to read AGENT.md");
            return;
        }
    };

    let messages: Vec<ChatMessage> = consolidation_messages
        .into_iter()
        .filter(|m| m.role != MessageRole::System)
        .collect();

    if messages.is_empty() {
        eprintln!("\x1b[90m[Memory] skipped — no consolidation messages\x1b[0m");
        return;
    }

    let mut registry = ToolRegistry::new();
    registry.register(ReadFile::new(&memory_dir));
    registry.register(WriteFile::new(&memory_dir));

    let tools: Vec<Arc<dyn ITool>> = registry
        .list()
        .into_iter()
        .cloned()
        .collect();

    let pipeline_client: Arc<dyn IChatClient> = Arc::new(
        FunctionInvokingChatClient::new(client, tools.clone()).with_max_rounds(200),
    );

    let agent = ChatClientAgent::new("memory-agent", pipeline_client)
        .with_instructions(agent_md)
        .with_tools(registry);

    let run_opts = AgentRunOptions::new()
        .with_thinking(false)
        .with_parallel_tool_calls(false);

    let stream = match agent.run(messages, None, Some(run_opts)).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "MemoryAgent failed to start");
            eprintln!("\x1b[31m[Memory] failed to start: {}\x1b[0m", e);
            return;
        }
    };

    let mut output = String::new();
    {
        let mut stream = Box::pin(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(r) => {
                    for c in &r.contents {
                        if let rust_agent_core::Content::Text(t) = c {
                            output.push_str(&t.delta);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "MemoryAgent stream error");
                    eprintln!("\x1b[31m[Memory] stream error: {}\x1b[0m", e);
                    return;
                }
            }
        }
    }

    let trimmed = output.trim();
    let gaps = scan_index_gaps(&memory_dir);
    let gap_text = format_index_gaps(&gaps);

    if !gap_text.is_empty() {
        eprintln!("\x1b[33m[Memory] {}\x1b[0m", gap_text);
    }

    if trimmed.is_empty() || trimmed == "OK" {
        if gap_text.is_empty() {
            eprintln!("\x1b[90m[Memory] OK\x1b[0m");
        }
    } else if trimmed.starts_with("UPDATED:") || trimmed.starts_with("INDEX_GAP:") {
        eprintln!("\x1b[32m[Memory] {}\x1b[0m", trimmed);
    } else {
        eprintln!("\x1b[32m[Memory] {}\x1b[0m", trimmed);
        tracing::info!(result = %trimmed, "MemoryAgent consolidation completed");
    }
}

/// Build consolidation messages from session projection + current turn transcript.
pub(crate) fn prepare_consolidation_messages(
    memory_projection: &[ChatMessage],
    turn_transcript: &[ChatMessage],
) -> Vec<ChatMessage> {
    build_consolidation_context(memory_projection, turn_transcript)
}

/// Legacy entry point — kept for tests; prefer `run_memory_agent` with projected messages.
#[allow(dead_code)]
pub(crate) async fn run_memory_agent_legacy(
    memory_dir: PathBuf,
    client: Arc<dyn IChatClient>,
    request_messages: Vec<ChatMessage>,
    response: Option<AgentResponse>,
) {
    let mut turn = request_messages
        .into_iter()
        .filter(|m| m.role != MessageRole::System)
        .collect::<Vec<_>>();
    if let Some(resp) = response {
        if !resp.turn_transcript.is_empty() {
            turn = resp.turn_transcript.clone();
        } else if !resp.tool_calls.is_empty() {
            turn.push(ChatMessage::assistant_with_tools(
                resp.text.clone(),
                resp.tool_calls.clone(),
            ));
            turn.extend(resp.tool_messages.clone());
        } else if !resp.text.is_empty() {
            turn.push(ChatMessage::assistant(resp.text));
        }
    }
    run_memory_agent(memory_dir, client, turn).await;
}

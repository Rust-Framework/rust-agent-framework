use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::{ChatMessage, IAgent, IChatClient, ITool, MessageRole, ToolRegistry};

use crate::tools::{ReadFile, WriteFile};
use crate::chat_client_decorators::FunctionInvokingChatClient;
use crate::ChatClientAgent;

/// 运行 MemoryAgent 进行记忆沉淀。
///
/// 读取 `AGENT.md` 作为 system prompt，构建一个带有 `ReadFile` 和 `WriteFile`
/// 工具的子代理，分析对话上下文并将有价值的信息写入持久记忆文件。
///
/// ## Working directory
///
/// Switches the process CWD to `memory_dir` before executing so that
/// relative paths (e.g. `references/USER.md`) in the LLM's tool calls
/// resolve against the correct memory directory.  The original CWD is
/// restored before returning.
pub(crate) async fn run_memory_agent(
    memory_dir: PathBuf,
    client: Arc<dyn IChatClient>,
    request_messages: Vec<ChatMessage>,
    response: Option<String>,
) {
    // Canonicalize before switching CWD — `memory_dir` may be a relative
    // path (e.g. "logs/memory").  After set_current_dir() the relative
    // path would resolve against the NEW cwd, doubling the path segment.
    let memory_dir = match memory_dir.canonicalize() {
        Ok(abs) => abs,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to canonicalize memory_dir");
            return;
        }
    };

    // Switch CWD to memory_dir so read_file / write_file paths resolve
    // correctly.  AGENT.md instructs the LLM to use relative paths like
    // `references/USER.md` — without this they would resolve against the
    // process startup CWD (workspace root), writing memory files to the
    // wrong location.
    let prev_cwd = std::env::current_dir().ok();
    if std::env::set_current_dir(&memory_dir).is_err() {
        tracing::warn!("Failed to set CWD to memory_dir");
    }
    let restore_cwd = || {
        if let Some(d) = &prev_cwd {
            let _ = std::env::set_current_dir(d);
        }
    };

    // 读取 AGENT.md 作为 system prompt
    let agent_md = match std::fs::read_to_string(memory_dir.join("AGENT.md")) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read AGENT.md");
            restore_cwd();
            return;
        }
    };

    // 构建 MemoryAgent 输入
    let mut input = agent_md.clone();
    input.push_str("\n\n---\n\n## 本轮对话上下文\n\n");
    for msg in &request_messages {
        if msg.role != MessageRole::System {
            input.push_str(&format!(
                "[{}]: {}\n",
                match msg.role {
                    MessageRole::User => "用户",
                    MessageRole::Assistant => "助手",
                    MessageRole::Tool => "工具",
                    _ => "其他",
                },
                msg.content
            ));
        }
    }
    if let Some(ref resp_text) = response {
        if !resp_text.is_empty() {
            input.push_str(&format!("\n[助手回复]: {}\n", resp_text));
        }
    }

    // 构建 MemoryAgent
    //
    // IMPORTANT: must wrap the client in FunctionInvokingChatClient so
    // tool calls (read_file / write_file) from the LLM are auto-invoked.
    // ChatClientAgent::new().with_tools() only stores tool *definitions*
    // in the registry; the execution loop comes from the decorator.
    let mut registry = ToolRegistry::new();
    registry.register(ReadFile);
    registry.register(WriteFile);

    let tools: Vec<Arc<dyn ITool>> = registry
        .list()
        .into_iter()
        .cloned()
        .collect();
    let pipeline_client: Arc<dyn IChatClient> = Arc::new(
        FunctionInvokingChatClient::new(client, tools.clone())
            .with_max_rounds(5),
    );

    let agent = ChatClientAgent::new("memory-agent", pipeline_client)
        .with_instructions(agent_md)
        .with_tools(registry);

    let messages = vec![ChatMessage::user(&input)];
    let stream = match agent.run(messages, None, None).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "MemoryAgent failed to start");
            restore_cwd();
            return;
        }
    };

    // 消费流并记录结果
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
                    restore_cwd();
                    return;
                }
            }
        }
    }

    let trimmed = output.trim();
    if !trimmed.is_empty() && trimmed != "OK" {
        tracing::info!(result = %trimmed, "MemoryAgent consolidation completed");
    }
    restore_cwd();
}

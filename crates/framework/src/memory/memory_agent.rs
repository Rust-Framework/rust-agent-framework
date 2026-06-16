use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::{ChatMessage, IAgent, IChatClient, MessageRole, ToolRegistry};

use crate::tools::{ReadFile, WriteFile};
use crate::ChatClientAgent;

/// 运行 MemoryAgent 进行记忆沉淀。
///
/// 读取 `AGENT.md` 作为 system prompt，构建一个带有 `ReadFile` 和 `WriteFile`
/// 工具的子代理，分析对话上下文并将有价值的信息写入持久记忆文件。
pub(crate) async fn run_memory_agent(
    memory_dir: PathBuf,
    client: Arc<dyn IChatClient>,
    request_messages: Vec<ChatMessage>,
    response: Option<String>,
) {
    // 读取 AGENT.md 作为 system prompt
    let agent_md = match std::fs::read_to_string(memory_dir.join("AGENT.md")) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to read AGENT.md");
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
    let mut registry = ToolRegistry::new();
    registry.register(ReadFile);
    registry.register(WriteFile);

    let agent = ChatClientAgent::new("memory-agent", client)
        .with_instructions(agent_md)
        .with_tools(registry);

    let messages = vec![ChatMessage::user(&input)];
    let stream = match agent.run(messages, None, None).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "MemoryAgent failed to start");
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
                    return;
                }
            }
        }
    }

    let trimmed = output.trim();
    if !trimmed.is_empty() && trimmed != "OK" {
        tracing::info!(result = %trimmed, "MemoryAgent consolidation completed");
    }
}

# rust-agent-framework

A modular, async-native Rust framework for building LLM-powered AI agents with streaming, tool-calling, and multi-agent orchestration — inspired by Microsoft Agent Framework (MAF).

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     rust-agent-cli                        │
│                   (Binary / Example)                      │
├─────────────────────────────────────────────────────────┤
│      rust-agent-workflow       rust-agent-framework       │
│   (Orchestration Patterns)     (Agent Runtime & Tools)    │
├─────────────────────────────────────────────────────────┤
│            rust-agent-client     rust-agent-macros        │
│          (LLM Provider Clients)   (Proc Macros)           │
├─────────────────────────────────────────────────────────┤
│                    rust-agent-core                        │
│        (Traits, Types, Streaming Infrastructure)          │
└─────────────────────────────────────────────────────────┘
```

### Crate Map

| Crate | Package Name | Role |
|---|---|---|
| [core](crates/core/) | `rust-agent-core` | Core traits (`IAgent`, `IChatClient`, `ITool`, `ISession`, `IContextProvider`), message/stream types, error system |
| [client](crates/client/) | `rust-agent-client` | LLM provider clients (OpenAI, DeepSeek), HTTP/SSE transport, usage parsing |
| [framework](crates/framework/) | `rust-agent-framework` | Agent runtime, `ChatClientAgent`, `ToolLoopAgent`, `AgentBuilder`, 13 built-in tools, context providers |
| [macros](crates/macros/) | `rust-agent-macros` | `#[tool]` proc-macro for ergonomic tool definitions |
| [workflow](crates/workflow/) | `rust-agent-workflow` | Graph-based workflow engine, patterns (Sequential, Concurrent, Handoff) |
| [cli](crates/cli/) | `rust-agent-cli` | Interactive CLI application demonstrating full framework usage |

## Key Features

- **Streaming-first** — every interface uses `BoxStream` for real-time token-by-token output
- **Auto tool-calling loop** — `ToolLoopAgent` intercepts tool calls, executes tools in parallel, and feeds results back
- **Complete tool call lifecycle** — 5-stage streaming lifecycle: `Start → Args(×N) → End → Calling → Called`
- **Provider-agnostic** — OpenAI and DeepSeek support out of the box, extendable via `IChatClient`
- **Context provider chain** — composable pre/post-invocation hooks for history management, RAG, compression
- **Workflow patterns** — sequential, concurrent (fan-out/fan-in), and handoff orchestration
- **13 built-in tools** — file I/O, shell commands, web search/fetch, directory operations
- **`#[tool]` macro** — single-attribute annotation to define tools with auto-generated JSON Schema
- **Streaming JSON arg parser** — real-time incremental parsing of tool arguments during streaming

## Quick Start

```rust
use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_core::{AgentSession, ChatMessage, ISession};
use rust_agent_framework::{tool, AgentBuilder};
use std::sync::Arc;

#[tool(description = "Echoes back the input text")]
async fn echo(#[param(desc = "Text to echo")] text: String) -> String {
    text
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = DeepSeekChatClient::new(
        ChatClientOptions::deepseek("deepseek-chat", "your-api-key")
    )?;

    let agent = AgentBuilder::new("my-agent")
        .chat_client(client)
        .instructions("You are a helpful assistant.")
        .with_tool(Echo)
        .build()?;

    let session = Arc::new(AgentSession::new());
    let messages = vec![ChatMessage::user("Hello!")];

    let mut stream = agent.run(messages, Some(session), None).await?;
    use futures_util::StreamExt;
    while let Some(Ok(chunk)) = stream.next().await {
        // Handle chunk.contents ...
    }
    Ok(())
}
```

## Built-in Tools

All tools are defined using the `#[tool]` macro and located in the `framework` crate:

| Tool | Description |
|---|---|
| `read_file` | Read file contents with line range support |
| `write_file` | Create or overwrite files |
| `edit_file` | Perform exact string replacements in files |
| `list_files` | List directory contents |
| `inspect_file` | Inspect file metadata and structure |
| `make_directory` | Create directories recursively |
| `remove_path` | Remove files or directories |
| `move_file` | Move/rename files |
| `find_files` | Find files by glob pattern |
| `search_file` | Search file contents with regex |
| `run_command` | Execute shell commands |
| `web_search` | Perform web searches |
| `web_fetch` | Fetch content from URLs |

## Streaming Tool Call Lifecycle

The framework exposes a 5-stage tool call lifecycle in the stream, enabling fine-grained UI feedback:

```
ToolCallStart → ToolCallArgs(×N) → ToolCallEnd → ToolCalling → ToolCalled
    ①              ②                 ③             ④             ⑤
  begins          streaming          args        complete       execution
                  fragments          done        invocation      result
```

Additional granular events:
- **ToolCallArgsParsed** — a parameter key-value pair is complete (emitted incrementally)
- **ToolCallArgsProgress** — a long string parameter is still arriving (live progress for UI)

## Project Structure

```
rust-agent-framework/
├── crates/
│   ├── core/         # Core traits, types, streaming
│   ├── client/       # LLM provider clients
│   ├── framework/    # Agent runtime + built-in tools
│   ├── macros/       # #[tool] proc-macro
│   ├── workflow/     # Orchestration patterns
│   └── cli/          # Interactive CLI binary
├── scripts/          # Publishing scripts
├── Cargo.toml        # Workspace root
└── LICENSE           # MIT
```

## Requirements

- Rust 1.80+
- Tokio async runtime

## License

MIT
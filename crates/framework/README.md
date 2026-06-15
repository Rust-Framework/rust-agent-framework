# rust-agent-framework

Agent runtime and orchestration layer — the core engine that assembles `IChatClient`, `ITool`, and `IContextProvider` into executable agents.

## Role

Implements `IAgent` with full lifecycle management including context provider chains, LLM invocation, streaming response conversion, and auto tool-calling loops. Provides 13 built-in tools and a fluent `AgentBuilder`.

## Components

### [`ChatClientAgent`](src/chat_client_agent.rs)

The primary `IAgent` implementation. Orchestrates a 3-phase pipeline:

1. **Pre-invocation** — runs `IContextProvider` chain in registration order. Each provider injects instructions, messages, and tools. Supports `replace_messages` for compression strategies.
2. **LLM invocation** — assembles `[system] + [provider_messages] + [caller_messages]`, merges tool definitions from registry and providers, calls `IChatClient::run()`.
3. **Post-invocation** — non-blocking channel-based fork. Reconstructs `AgentResponse`, calls `on_invoked()` on each provider, persists assistant text to session.

The converter (`AgentResponseConverter`) maps internal `AgentResponseUpdate` deltas to public `AgentResponseResult` chunks with full tool call lifecycle support.

### [`ToolLoopAgent`](src/agents/tool_loop_agent.rs)

Wraps any inner `IAgent` and implements the auto tool-calling loop:

1. Calls inner agent, forwarding text deltas in real time (typing effect)
2. Buffers tool call chunks until stream ends
3. Executes all tools **in parallel** via `join_all()`
4. Persists `assistant(tool_calls)` + `tool(result)` messages to session
5. Feeds results back as new messages for the next loop iteration
6. Capped at `max_rounds` (default: 10)

State machine uses `futures_util::stream::unfold` with `LoopState::{Looping, Streaming, Done}`.

### [`AgentBuilder`](src/builder.rs)

Fluent builder that constructs the agent stack:

```rust
let agent: Arc<dyn IAgent> = AgentBuilder::new("my-agent")
    .chat_client(client)
    .instructions("You are a helpful assistant.")
    .with_tool(Echo)
    .with_tool(Add)
    .max_tool_rounds(5)
    .add_context_provider(CustomProvider::new())
    .build()?;
```

Build process:
1. Creates `ChatClientAgent` with instructions, tools, context providers
2. If tools are present, wraps in `ToolLoopAgent`
3. Returns `Arc<dyn IAgent>`

Defaults:
- `InMemoryHistoryProvider` is always the first context provider
- `max_tool_rounds` defaults to 10

Key methods:
- `with_history_provider()` — replace the built-in history provider (e.g. with Redis-backed)
- `add_context_provider()` — append a provider to the chain after history
- `with_tool()` — add a tool to the registry

### [`AgentRuntime`](src/agent_runtime.rs)

Agent host for registration and message routing:

```rust
let mut runtime = AgentRuntime::new();
runtime.register_agent(agent);
let stream = runtime.run(&AgentId::new("my-agent"), messages, session, options).await?;
```

### [`AgentResponseConverter`](src/converter.rs)

Converts internal SSE-level `AgentResponseUpdate` deltas to public `AgentResponseResult` chunks. Features:

- **Parallel tool call support** — accumulators keyed by `call_id`, not position index
- **Lifecycle decomposition** — legacy `ToolCallDelta` decomposed into `Start/Args/End` for consistent API
- **Duplicate prevention** — deduplicates `ToolCallStart` from explicit + decomposed sources
- **Streaming arg parser integration** — feeds `ToolCallArgs` deltas into `StreamingArgsParser`, emits `ToolCallArgsParsed` and `ToolCallArgsProgress` in real time
- **Auto-flush** — emits pending `ToolCallEnd` on stream termination, flushes accumulated calls as `ToolCallingContent` on `FinishReason::ToolCalls`

### [`InMemoryHistoryProvider`](src/context_providers/history_provider.rs)

Default `IContextProvider` that manages conversation history:

- `on_invoking` — loads all messages from session
- `on_invoked` — atomically batch-persists new messages, deduplicating by tracking message count

## Built-in Tools

All 13 tools are defined with the `#[tool]` macro:

```rust
use rust_agent_framework::tools::register_all;

let mut registry = ToolRegistry::new();
register_all(&mut registry);
```

| Tool | Source | Description |
|---|---|---|
| `read_file` | [read_file.rs](src/tools/read_file.rs) | Read file with offset/limit (max 512KB) |
| `write_file` | [write_file.rs](src/tools/write_file.rs) | Create/overwrite files, auto-creates parents |
| `edit_file` | [edit_file.rs](src/tools/edit_file.rs) | Exact string replacement in files |
| `list_files` | [list_files.rs](src/tools/list_files.rs) | List directory contents |
| `inspect_file` | [inspect_file.rs](src/tools/inspect_file.rs) | File metadata and structure |
| `make_directory` | [make_directory.rs](src/tools/make_directory.rs) | Recursive directory creation |
| `remove_path` | [remove_path.rs](src/tools/remove_path.rs) | Remove files or directories |
| `move_file` | [move_file.rs](src/tools/move_file.rs) | Move/rename files |
| `find_files` | [find_files.rs](src/tools/find_files.rs) | Glob pattern file search |
| `search_file` | [search_file.rs](src/tools/search_file.rs) | Regex content search in files |
| `run_command` | [run_command.rs](src/tools/run_command.rs) | Shell command execution (100KB output cap) |
| `web_search` | [web_search.rs](src/tools/web_search.rs) | Web search |
| `web_fetch` | [web_fetch.rs](src/tools/web_fetch.rs) | URL content fetching |

All tools return `{"ok": bool, "data": ..., "error": ...}` JSON format.

## Context Providers

The `IContextProvider` chain is the framework's primary extension mechanism:

```rust
// Compression provider example
impl IContextProvider for CompressionProvider {
    async fn on_invoking(&self, ...) -> Result<ContextInjection> {
        Ok(ContextInjection {
            messages: summarize_and_truncate(session.get_messages().await?),
            replace_messages: true,  // replaces history with compressed version
            ..Default::default()
        })
    }
}
```

Chain execution order: `[InMemoryHistory, RAG, Skills, Compression, ...]`

## Re-exports

The framework crate re-exports `rust_agent_macros::tool` so users only need one dependency:

```rust
// Single import:
use rust_agent_framework::tool;
use rust_agent_framework::AgentBuilder;
```

## Dependencies

- `rust-agent-core` — traits and types
- `rust-agent-client` — LLM provider clients (for `IChatClient`)
- `rust-agent-macros` — `#[tool]` proc-macro
- `regex`, `glob`, `walkdir` — file search tools
- `reqwest` — web fetch tool
- `tarzi` — tar/archive inspection
- `chrono` — timestamps
- `tokio`, `futures-core`, `futures-util` — async runtime
- `serde`, `serde_json` — serialization
- `async-trait`, `anyhow`, `tracing` — utilities
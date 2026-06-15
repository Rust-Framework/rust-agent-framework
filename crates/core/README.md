# rust-agent-core

Core abstractions layer — the foundation of the rust-agent-framework ecosystem. Defines all traits, types, streaming infrastructure, and the error system shared across every other crate.

## Role

- Defines framework-level traits (`IAgent`, `IChatClient`, `ITool`, `ISession`, `IContextProvider`)
- Defines shared data structures (`ChatMessage`, `AgentResponse`, `AgentResponseResult`, `ToolCall`, `Usage`, etc.)
- Provides streaming primitives (`BoxStream<T>`, `collect_agent_response`)
- Provides default implementations (`AgentSession`, `ToolRegistry`)
- Defines the unified error type (`AgentError`)

## Public API

### Core Traits

| Trait | Purpose |
|---|---|
| [`IAgent`](src/agent.rs) | Autonomous software component that reasons and executes using LLMs and tools. Supports sub-agent lookup. |
| [`IChatClient`](src/chat_client.rs) | Thin wrapper over LLM provider APIs. Streaming-only. |
| [`ITool`](src/tool.rs) | Abstraction for callable tools with JSON Schema parameters. |
| [`ISession`](src/session.rs) | Multi-turn conversation state manager with KV cache tracking. |
| [`IContextProvider`](src/context_provider.rs) | Pre/post-invocation hook for injecting context (history, RAG, compression). |

### Key Types

| Type | Description |
|---|---|
| `ChatMessage` | Unified message with `role`, `content`, `tool_calls`, `tool_call_id`. Convenience constructors: `system()`, `user()`, `assistant()`, `assistant_with_tools()`, `tool()`. |
| `AgentResponse` | Aggregated response: `text`, `reasoning_text`, `tool_calls`, `usage`, `source_agent_id`. |
| `AgentResponseResult` | Streamed chunk: `contents: Vec<Content>`, `events: Vec<Event>`, `finish_reason`. |
| `AgentResponseUpdate` | Internal SSE-level delta enum (12 variants). |
| `Content` | 12-variant enum covering text, reasoning, tool call lifecycle (5 stages), usage, errors. |
| `Event` | Executor lifecycle events (`ExecutorInvoking`, `ExecutorInvoked`, `Custom`). |
| `ToolCall` | A tool invocation: `id`, `name`, `arguments`. |
| `ToolRegistry` | In-memory tool registry with `register()`, `get()`, `list()`. |
| `AgentSession` | Default `ISession` implementation using `RwLock<Vec<ChatMessage>>` with serialization. |
| `ResponseMetadata` | Per-chunk metadata: `agent_id`, `model_id`, `executor_id`, `timestamp`, `properties`. |
| `Usage` | Token usage with KV cache statistics and `cache_hit_ratio()` helper. |
| `FinishReason` | `Stop`, `Length`, `ToolCalls`, `ContentFilter`, or custom. |
| `AgentError` | Unified error enum: `ChatClientError`, `ToolError`, `WorkflowError`, `SessionError`, `ConfigError`, `AgentNotFound`, `StreamError`, `Serialize`, `Other(anyhow)`. |
| `ReasoningEffort` | `High` / `Max` — controls model reasoning depth. |

### Streaming Infrastructure

```rust
pub type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

pub async fn collect_agent_response(
    stream: BoxStream<'static, Result<AgentResponseResult>>,
) -> Result<AgentResponse>;
```

### Incremental JSON Parser

[`StreamingArgsParser`](src/incremental_json.rs) — character-level state machine that parses tool-call JSON as it streams in, emitting `ArgsEvent::Parsed` and `ArgsEvent::Progress` events in real time. O(n) total, no re-parsing.

### Agent Run Options

[`AgentRunOptions`](src/run_options.rs) — per-call overrides: `instructions`, `max_tokens`, `temperature`, `top_p`, `stop`, `extra_body`, `with_thinking(bool)`, `with_reasoning_effort(...)`, `parallel_tool_calls`.

Converts to `ChatClientRunOptions` for the transport layer via `to_chat_client_run_options()`.

## Design Principles

- **Trait-only abstraction** — no concrete LLM provider or HTTP logic lives here
- **Streaming-first** — every interface returns `BoxStream`, no blocking calls
- **I-prefix naming** — follows MAF conventions (`IAgent`, `IChatClient`, etc.)
- **Zero provider dependencies** — no `reqwest`, no API keys, no HTTP in this crate

## Usage

```rust
use rust_agent_core::{
    ChatMessage, AgentSession, ToolRegistry, BoxStream,
    IAgent, IChatClient, ITool, ISession, AgentError,
};
```

## Dependencies

- `async-trait`, `futures-core`, `futures-util` — async trait and stream support
- `serde`, `serde_json` — serialization
- `tokio` (sync feature) — `RwLock` for `AgentSession`
- `chrono` — timestamps in `ResponseMetadata` and `SessionMetadata`
- `uuid` — session ID generation
- `thiserror`, `anyhow` — error handling
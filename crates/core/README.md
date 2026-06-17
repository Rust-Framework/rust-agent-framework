# rust-agent-core

Core abstractions layer — the foundation of the rust-agent-framework ecosystem. Defines all traits, types, streaming infrastructure, and the error system shared across every other crate.

## Role

- Defines framework-level traits (`IAgent`, `IChatClient`, `ITool`, `ISession`, `IContextProvider`, `ITokenCounter`, `ICompressionStrategy`, `ISessionStore`)
- Defines shared data structures (`ChatMessage`, `AgentResponse`, `AgentResponseResult`, `ToolCall`, `Usage`, `AgentMetadata`, `ModelMetadata`, etc.)
- Provides streaming primitives (`BoxStream<T>`, `collect_agent_response`)
- Provides default implementations (`AgentSession`, `ToolRegistry`)
- Provides chat client infrastructure (`ChatClientBuilder`, `DelegatingChatClient`, `ChatClientRunOptions`)
- Defines the unified error type (`AgentError`)
- Provides incremental JSON parsing (`StreamingArgsParser`)
- Provides tool approval mechanism (`ApprovalRequiredTool`, `ToolApprovalResponse`)

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
| `Content` | 12-variant enum covering text, reasoning, tool call lifecycle (5 stages), usage, errors, and URI links. |
| `Event` | Executor lifecycle events (`ExecutorInvoking`, `ExecutorInvoked`, `Custom`). |
| `ToolCall` | A tool invocation: `id`, `name`, `arguments`. |
| `ToolApprovalResponse` | Approval decision for a tool call: `call_id`, `approved`, `reason`. |
| `ToolRegistry` | In-memory tool registry with `register()`, `get()`, `list()`. |
| `AgentSession` | Default `ISession` implementation using `RwLock<Vec<ChatMessage>>` with serialization. |
| `AgentId` | Newtype wrapper around `String` for agent identification. |
| `AgentMetadata` | Agent metadata: `agent_type`, `description`, `model_id`, `tool_names`, `capability_tags`. |
| `ModelMetadata` | LLM model metadata: `context_window`, `max_output_tokens`, `supports_reasoning`. |
| `ResponseMetadata` | Per-chunk metadata: `agent_id`, `model_id`, `executor_id`, `timestamp`, `properties`. |
| `Usage` | Token usage with KV cache statistics and `cache_hit_ratio()` helper. |
| `FinishReason` | `Stop`, `Length`, `ToolCalls`, `ContentFilter`, `AwaitingApproval`, or custom. |
| `AgentError` | Unified error enum: `ChatClientError`, `ToolError`, `WorkflowError`, `SessionError`, `ConfigError`, `AgentNotFound`, `StreamError`, `Serialize`, `Other(anyhow)`. |
| `ReasoningEffort` | `High` / `Max` — controls model reasoning depth. |

### Chat Client Infrastructure

| Type | Description |
|---|---|
| `IChatClient` | Thin wrapper over LLM provider APIs. Streaming-only. |
| `ChatClientBuilder` | Builder pattern for composing chat client decorators (function invoking, per-call persistence). |
| `DelegatingChatClient` | Decorator base that delegates to an inner `IChatClient`, enabling composable wrappers. |
| `ChatClientRunOptions` | Per-call options passed to chat clients (temperature, max_tokens, tools, extra_body). |

### Session Management

| Type | Description |
|---|---|
| `ISession` | Multi-turn conversation state manager with KV cache tracking and serialization. |
| `SessionMetadata` | Session metadata (`id`, `created_at`, `last_active_at`, `message_count`). |
| `SessionSnapshot` | Full session snapshot for serialization. |
| `ProviderState` | Per-provider KV cache state. |
| `ProviderStateStore` | Map of provider states keyed by provider name. |
| `SessionTTLOptions` | TTL configuration: `max_idle_secs`, `max_lifetime_secs`, `cleanup_interval_secs`. |
| `ISessionStore` | Persistence trait for sessions: `save_session()`, `get_session()`, `remove_session()`, `list_sessions()`. |

### Additional Traits

| Trait | Purpose |
|---|---|
| `ITokenCounter` | Estimates token counts for messages. Used by compression strategies. |
| `ICompressionStrategy` | Strategy trait for compressing message history to fit within context windows. |
| `ApprovalRequiredTool` | Wrapper that marks a tool as requiring human approval before execution. |

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
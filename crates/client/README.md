# rust-agent-client

LLM provider client implementations — the transport layer between the framework and external LLM APIs.

## Role

Implements `IChatClient` for specific LLM providers. Handles HTTP communication, SSE stream parsing, request/response serialization, and provider-specific usage statistics.

## Provider Clients

### [`ChatClient`](src/chat_client.rs) — Generic base

The shared HTTP/SSE transport engine used by all provider clients. Features:

- POST to `{api_base}/chat/completions` with JSON body
- SSE stream parsing via `SseStream`
- Request body construction: messages, model, streaming flags, tool definitions
- Per-call option overrides (temperature, max_tokens, extra_body, tools)
- Error handling for HTTP errors and parse failures

### [`ModelListEntry`](src/types.rs)

Returned by provider-specific `list_models()` calls:

```rust
pub struct ModelListEntry {
    pub id: String,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}
```

### [`OpenAiChatClient`](src/openai_client.rs)

OpenAI API wrapper. Composes `ChatClient` with `UsageFormat::OpenAI` for usage parsing.

```rust
let client = OpenAiChatClient::new(
    ChatClientOptions::openai("gpt-4o", "sk-...")
)?;

// List available models
let models = client.list_models().await?;
```

Usage parsing specifics:
- Cache hits: `prompt_tokens_details.cached_tokens`
- Reasoning tokens: `completion_tokens_details.reasoning_tokens`

### [`DeepSeekChatClient`](src/deepseek_client.rs)

DeepSeek API wrapper. Composes `ChatClient` with `UsageFormat::DeepSeek`.

```rust
let client = DeepSeekChatClient::new(
    ChatClientOptions::deepseek("deepseek-chat", "sk-...")
)?;
```

DeepSeek-specific features:
- Base URL: `https://api.deepseek.com` (no `/v1` prefix)
- Thinking mode via `AgentRunOptions::with_thinking(true)`
- `reasoning_content` deltas in SSE stream
- KV cache stats: `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` at top level

## Configuration

[`ChatClientOptions`](src/options.rs) — static client configuration set at creation time:

| Field | Description |
|---|---|
| `api_base` | Full base URL (e.g. `https://api.openai.com/v1`, `https://api.deepseek.com`) |
| `api_key` | API key (serde-skipped to prevent leakage) |
| `model` | Model identifier |
| `max_tokens` | Default max tokens (per-call overridable) |
| `temperature` | Default temperature (per-call overridable) |
| `top_p` | Default top_p (per-call overridable) |
| `stop` | Default stop sequences (per-call overridable) |
| `extra_headers` | Extra HTTP headers (e.g. `OpenAI-Organization`) |
| `timeout_secs` | Request timeout (default: 60s) |

Convenience constructors:
- `ChatClientOptions::openai(model, api_key)`
- `ChatClientOptions::deepseek(model, api_key)`

## Transport Layer

[`SseStream`](src/transport.rs) — custom `Stream` implementation that:

1. Reads raw bytes from `reqwest` byte stream
2. Buffers and splits on newlines
3. Parses `data: {...}` SSE lines into `SseChunk` structs
4. Maps chunks to `AgentResponseUpdate` events (text deltas, reasoning deltas, tool call deltas, usage, finish)
5. Yields events via FIFO: first event returned, rest queued in `pending`

Handles both OpenAI and DeepSeek SSE formats, including:
- `[DONE]` termination signal
- `finish_reason` mapping (`stop` → `FinishReason::Stop`, `tool_calls` → `FinishReason::ToolCalls`, etc.)
- Provider-specific usage parsing via `UsageFormat`

## Usage Format

[`usage.rs`](src/usage.rs) — per-provider usage deserialization with independent structs:

| Format | Cache Tokens | Reasoning Tokens |
|---|---|---|
| `UsageFormat::OpenAI` | `prompt_tokens_details.cached_tokens` | `completion_tokens_details.reasoning_tokens` |
| `UsageFormat::DeepSeek` | `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` (top-level) | `completion_tokens_details.reasoning_tokens` |

All formats normalize into `rust_agent_core::Usage` via `into_usage()`.

## Extending with New Providers

To add a new provider:

1. Create a new struct composing `ChatClient`
2. Implement `IChatClient` — delegate to `inner.chat_stream(messages, &options, UsageFormat::...)`
3. Add a new `UsageFormat` variant and corresponding deserialization struct if needed

## Dependencies

- `rust-agent-core` — traits and types
- `reqwest` — HTTP client with `stream`, `json`, `rustls-tls` features
- `bytes` — buffered byte stream
- `serde`, `serde_json` — serialization
- `tokio`, `futures-core`, `futures-util` — async runtime
- `async-trait` — async trait support
- `tracing` — debug logging
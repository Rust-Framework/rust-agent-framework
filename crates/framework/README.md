# rust-agent-framework

Agent runtime and orchestration layer — the core engine that assembles `IChatClient`, `ITool`, and `IContextProvider` into executable agents.

## Role

Implements `IAgent` with full lifecycle management including context provider chains, LLM invocation, streaming response conversion, and auto tool-calling via ChatClient pipeline decorators. Provides 13 built-in tools and a fluent `AgentBuilder`.

## Components

### [`ChatClientAgent`](src/chat_client_agent.rs)

The primary `IAgent` implementation. Orchestrates a 3-phase pipeline:

1. **Pre-invocation** — runs `IContextProvider` chain in registration order. Each provider injects instructions, messages, and tools. Supports `replace_messages` for compression strategies.
2. **LLM invocation** — assembles `[system] + [provider_messages] + [caller_messages]`, merges tool definitions from registry and providers, calls `IChatClient::run()`.
3. **Post-invocation** — non-blocking channel-based fork. Reconstructs `AgentResponse`, calls `on_invoked()` on each provider, persists assistant text to session.

The converter (`AgentResponseConverter`) maps internal `AgentResponseUpdate` deltas to public `AgentResponseResult` chunks with full tool call lifecycle support.

### [`FunctionInvokingChatClient`](src/chat_client_decorators/function_invoking.rs)

ChatClient pipeline decorator implementing the auto tool-calling loop following MAF's `FunctionInvokingChatClient` pattern:

1. Calls inner `IChatClient`, forwarding text deltas in real time (typing effect)
2. Accumulates `ToolCallStart`/`ToolCallArgs`/`ToolCallEnd` streaming events
3. Executes all tools **in parallel** via `join_all()`
4. Builds accumulated messages (assistant tool_calls + tool results) for the next iteration
5. Feeds results back as new messages for the next loop iteration via `msg_tx`/`msg_rx` channel
6. Capped at `max_rounds` (default: 10)
7. Filters out internal `Finish(ToolCalls)` signals from consumer output

State machine uses `futures_util::stream::unfold` with `LoopState::{Looping, Streaming, Done}`.

### [`PerServiceCallPersistingChatClient`](src/chat_client_decorators/per_service_call_persisting.rs)

ChatClient pipeline decorator that triggers persistence after each LLM service call, ensuring intermediate state is saved during tool loops.

### [`AgentBuilder`](src/builder.rs)

Fluent builder that constructs the agent with ChatClient pipeline:

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
2. If tools are present, wraps `IChatClient` in `FunctionInvokingChatClient` via `ChatClientBuilder` pipeline
3. Returns `Arc<dyn IAgent>`

Defaults:
- `InMemoryHistoryProvider` is always the first context provider
- `max_tool_rounds` defaults to 10

Key methods:
- `with_history_provider()` — replace the built-in history provider (e.g. with Redis-backed)
- `add_context_provider()` — append a provider to the chain after history
- `with_tool()` — add a tool to the registry
- `with_compression_strategy()` — configure context compression
- `with_token_counter()` — configure token counting for compression

### [`AgentHost`](src/agent_host.rs)

Session registry and lifecycle management following MAF's `AIHostAgent` pattern:

```rust
let host = AgentHost::new(agent, session_store);
let session = host.get_or_create_session("conv-123").await?;
let stream = host.run(messages, session, None).await?;
```

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

## Agent Skills

Skills are portable packages of instructions, scripts, and resources based on the [Agent Skills open standard](https://agentskills.io/). They follow a **progressive disclosure** pattern — agents load only the context they need, when they need it.

### Skill Structure

A skill is a directory with a `SKILL.md` file and optional subdirectories:

```
my-skill/
├── SKILL.md            # Required — YAML frontmatter + Markdown instructions
├── scripts/            # Optional — executable scripts (.py, .js, .sh, .ps1)
├── references/         # Optional — reference documents loaded on demand
└── assets/             # Optional — templates and static resources
```

**SKILL.md format:**

```yaml
---
name: my-skill                  # Required: lowercase, numbers, hyphens (max 64)
description: What this does.    # Required: 1-1024 chars, include usage keywords
license: MIT                    # Optional
metadata:                       # Optional
  author: team-name
  version: "1.0"
---

# Instructions (Markdown — max 500 lines recommended)
1. Step one...
2. Reference materials in `references/` using `read_skill_resource`
3. Execute scripts via `run_skill_script`
```

### Progressive Disclosure

Skills minimize context usage through 4 stages:

| Stage | Trigger | Action | Tokens |
|-------|---------|--------|--------|
| **Advertise** | Every agent call | Inject skill names + descriptions into system prompt | ~100/skill |
| **Load** | LLM calls `load_skill(name)` | Return full SKILL.md instructions | <5000 |
| **Read** | LLM calls `read_skill_resource(name, path)` | Return reference document content | On-demand |
| **Run** | LLM calls `run_skill_script(name, path, args)` | Execute bundled script | On-demand |

### Usage

**File-based skills** (from directories):

```rust
use rust_agent_framework::{AgentSkillsProvider, AgentSkill};

// Scan a directory for all skills
let provider = AgentSkillsProvider::scan("./skills")?;

// Or load individual skills
let provider = AgentSkillsProvider::new()
    .with_skill(AgentSkill::from_dir("./skills/code-review")?)
    .with_skill(AgentSkill::from_dir("./skills/git-ops")?);

let agent = AgentBuilder::new("my-agent")
    .chat_client(client)
    .instructions("You are a helpful assistant.")
    .add_context_provider(provider)  // ← uses existing API
    .build()?;
```

**Dynamic skills** (database, API, etc.):

```rust
let skill = AgentSkill::dynamic(
    SkillMetadata {
        name: "enterprise-policy".into(),
        description: "Company expense policy rules.".into(),
        ..Default::default()
    },
    || db.query_instructions("enterprise-policy"),  // lazy loader
).with_resource("policy.pdf", || db.query_resource("policy.pdf"));

let provider = AgentSkillsProvider::new().with_skill(skill);
```

**Declarative (JSON):**

```json
{
  "id": "dev-assistant",
  "skill_directories": ["./company-skills"],
  "context_providers": [
    { "type": "skills", "names": ["code-review", "git-ops"] }
  ]
}
```

### Script Execution

Scripts in `scripts/` are executed by `SubprocessScriptRunner`, which auto-detects interpreters by extension:

| Extension | Interpreter |
|-----------|-------------|
| `.py` | `python` |
| `.js` | `node` |
| `.sh` | `bash` (Windows) / `sh` (Unix) |
| `.ps1` | `powershell -File` |
| other | `cmd /c` (Windows) / `sh -c` (Unix) |

Enable script execution by attaching a runner:

```rust
use rust_agent_framework::SubprocessScriptRunner;
use std::sync::Arc;

let provider = AgentSkillsProvider::scan("./skills")?
    .with_script_runner(Arc::new(SubprocessScriptRunner));
```

For sandboxed execution, implement `AgentSkillScriptRunner` with your own isolation logic.

### Best Practices

- **Keep SKILL.md under 500 lines**. Move detailed reference material to `references/`.
- **Write actionable descriptions**. Include keywords that help the LLM identify when to use the skill. Example: `"Use when asked to review code, check code quality, or evaluate pull requests."`
- **Name skills with lowercase + hyphens**: `code-review`, `git-ops`, `data-analysis`.
- **Use progressive disclosure**: Put only essential instructions in SKILL.md. Offload detailed rules, checklists, and templates to `references/`.
- **One skill, one domain**: Each skill should cover a single coherent task. Compose multiple skills rather than creating monolithic ones.
- **Scripts should be self-contained**: Pass all inputs via command-line arguments. Return structured output (JSON preferred) for reliable parsing by the LLM.
- **Resources are loaded on-demand only**: Large reference files consume zero tokens until the LLM explicitly calls `read_skill_resource`.

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

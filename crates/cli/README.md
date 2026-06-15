# rust-agent-cli

Interactive CLI binary demonstrating full rust-agent-framework usage. Serves as both an example and a practical starting point for building agent applications.

## Role

Assembles all framework crates into a runnable chat application with:

- Interactive REPL with line editing (rustyline)
- Tool-calling agent with DeepSeek backend
- Streaming output with rich terminal formatting
- Runtime model switching
- Thinking/reasoning mode toggle
- Full tool call lifecycle visualization
- Usage statistics display

## Features

### REPL Commands

| Command | Description |
|---|---|
| `/help` | Show available commands |
| `/clear` | Clear conversation history and reset agent |
| `/think on` / `/think off` | Enable/disable DeepSeek thinking (reasoning) mode |
| `/model <name>` | Switch model at runtime (e.g. `deepseek-chat`, `deepseek-reasoner`) |
| `/quit`, `/exit`, `quit`, `exit` | Exit the application |

### Streaming Display

The CLI renders the full tool call lifecycle with ANSI-colored output:

| Content Type | Display Format |
|---|---|
| `Text` | Inline text output (real-time typing) |
| `Reasoning` | Gray `[思考]` prefix for reasoning output |
| `ToolCallStart` | Cyan `[调用] tool_name` with parameter header |
| `ToolCallArgsParsed` | Green `param_name = value` per completed parameter |
| `ToolCallArgsProgress` | Gray progress indicator with KB counter |
| `ToolCalling` | Yellow `[参数] tool_name args_json` (compact summary) |
| `ToolCalled` | Green `[结果]` with result preview, Red `[结果] 失败` on error |
| `Usage` | Gray `[用量] prompt=N cache=... completion=N total=N` |
| `Error` | Red `[错误] code: message` |
| `FinishReason::ToolCalls` | Implicit — followed by tool lifecycle events |
| `ExecutorInvoking` / `ExecutorInvoked` | Gray `[调度]` lifecycle events with duration |

### Tool Demonstrations

Two example tools defined with `#[tool]`:

```rust
#[tool(description = "Echoes back the input text")]
async fn echo(#[param(desc = "The text to echo")] text: String) -> String { ... }

#[tool(description = "Adds two numbers together")]
async fn add(#[param(desc = "First number")] a: i64, #[param(desc = "Second number")] b: i64) -> String { ... }
```

## Usage

```bash
# Build and run
cargo run -p rust-agent-cli

# Or from workspace root
cargo run --bin rust-agent-cli
```

```text
rust-agent-cli — Interactive Chat (DeepSeek)
Type /help for commands, /quit to exit.

> Hello, what can you do?
[Text output streams here...]
```

## Architecture

```
User Input (REPL)
    │
    ▼
AgentBuilder::new("cli-agent")
    ├── DeepSeekChatClient (deepseek-v4-flash)
    ├── instructions: "You are a helpful AI assistant..."
    └── tools: [Echo, Add]
    │
    ▼
agent.run(messages, session, run_options)
    │
    ▼
Stream Consumer Loop
    ├── Content::Text        → print!()
    ├── Content::Reasoning   → print!("\x1b[90m[思考] ...")
    ├── ToolCallStart        → eprintln!("\x1b[36m[调用] ...")
    ├── ToolCallArgsParsed   → eprintln!("  param = value")
    ├── ToolCallArgsProgress → eprintln!("  ... (KB)")
    ├── ToolCallEnd          → (hidden)
    ├── ToolCalling          → eprintln!("\x1b[33m[参数] ...")
    ├── ToolCalled           → eprintln!("\x1b[32m[结果] ...")
    └── Usage                → eprintln!("\x1b[90m[用量] ...")
```

## Configuration

The API key is hardcoded for development purposes. Replace `DEEPSEEK_API_KEY` in [main.rs](src/main.rs) with your own key before use.

## Dependencies

- `rust-agent-core` — types and session
- `rust-agent-framework` — `AgentBuilder`, `tool` macro
- `rust-agent-client` — `DeepSeekChatClient`
- `rust-agent-workflow` — workflow crate (available for import)
- `tokio` — async runtime
- `tracing-subscriber` — logging (warn level in chat mode)
- `rustyline` — readline-style line editing with history
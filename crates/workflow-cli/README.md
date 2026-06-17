# rust-agent-workflow-cli

Workflow orchestration CLI test program — full pipeline verification with trace logging for the `rust-agent-workflow` crate.

## Overview

A comprehensive integration test binary that exercises all major workflow features using real LLM calls. Outputs structured trace logs to `logs/workflow-cli.log` for debugging and verification.

## Test Scenarios

The binary runs 8 ordered test scenarios:

| # | Scenario | What It Tests |
|---|---|---|
| 1 | **Session direct management** | `IAgent::run()` with session persistence, `get_subagent()` discovery, `InMemorySessionStore` save/load, streaming output |
| 2 | **WorkflowEngine + Checkpoint** | `WorkflowEngine::run()` with `CheckpointManager`, event stream consumption, SuperStep execution |
| 3 | **Session TTL cleanup** | `InMemorySessionStore` with TTL options, idle expiration, automatic cleanup |
| 4 | **Tool call pipeline** | `FunctionInvokingChatClient` wrapping, `ReadFile` tool execution, multi-turn tool use |
| 5 | **Handoff routing** | `HandoffWorkflow` triage routing, `as_agent()` facade, `get_subagent()` sub-agent discovery, real LLM routing |
| 6 | **Sequential orchestration** | `SequentialWorkflow` — researcher → summarizer pipeline, output propagation |
| 7 | **Concurrent orchestration** | `ConcurrentWorkflow` — parallel multi-perspective analysis, merged stream |
| 8 | **Sub-agent independent streaming** | `as_agent()` → `get_subagent()` → sub-agent `run()` with streaming, parent triage verification |

## Usage

```bash
# Basic run (requires DeepSeek API key configured in source)
cargo run -p rust-agent-workflow-cli

# With debug-level trace logs
RUST_LOG=debug cargo run -p rust-agent-workflow-cli

# With trace-level logs (very verbose)
RUST_LOG=trace cargo run -p rust-agent-workflow-cli
```

## Output

```
╔══════════════════════════════════════════╗
║  Workflow CLI — 流程编排测试程序          ║
║  日志输出: logs/workflow-cli.log           ║
╚══════════════════════════════════════════╝
▶ 场景1: Session直接管理  ✅
▶ 场景2: WorkflowEngine+Checkpoint  ✅
▶ 场景3: Session TTL cleanup  ✅
▶ 场景4: 工具调用管道  ✅
▶ 场景5: Handoff 路由编排  ✅
▶ 场景6: Sequential 顺序编排  ✅
▶ 场景7: Concurrent 并发编排  ✅
▶ 场景8: as_agent→get_subagent→流式输出  ✅

╔══════════════════════════════════════════╗
║  结果: 8/8 通过, 0 失败
║  完整日志: logs/workflow-cli.log
║  ✅ 所有场景通过！
╚══════════════════════════════════════════╝
```

## Configuration

The API key and model are hardcoded in `src/main.rs`:

```rust
const DEEPSEEK_API_KEY: &str = "sk-...";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";
```

Replace these with your own credentials before running.

## Dependencies

| Crate | Purpose |
|---|---|
| `rust-agent-core` | Types (`IAgent`, `ChatMessage`, `ISession`, `ISessionStore`) |
| `rust-agent-framework` | `AgentBuilder`, `InMemorySessionStore`, built-in tools |
| `rust-agent-client` | `DeepSeekChatClient`, `ChatClientOptions` |
| `rust-agent-workflow` | `SequentialWorkflow`, `HandoffWorkflow`, `ConcurrentWorkflow`, `WorkflowEngine`, `CheckpointManager` |
| `tokio` | Async runtime |
| `tracing` / `tracing-subscriber` | Structured trace logging |
| `tracing-appender` | Non-blocking file log writer |

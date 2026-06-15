# rust-agent-workflow

Multi-agent orchestration layer — graph-based workflow engine and reusable orchestration patterns.

## Role

Implements multi-agent coordination through a graph-based engine (`GraphFlow`) and composable patterns (Sequential, Concurrent, Handoff).

## Components

### [`GraphFlow`](src/graph_flow.rs)

A graph-based workflow engine. Implements `IAgent` so it can be used as a sub-agent in larger workflows.

```rust
let mut flow = GraphFlow::new();
flow.add_agent(researcher);
flow.add_agent(writer);
flow.set_entry(AgentId::new("researcher"));

let stream = flow.run(messages, session, options).await?;
// Or start from a specific agent:
let stream = flow.run_from(&AgentId::new("writer"), messages, session, options).await?;
```

Current state: MVP — supports agent registration and entry-point execution. Edge conditions and multi-step graph traversal are under development.

### [`SequentialPattern`](src/patterns/sequential.rs)

Runs agents in order, piping each agent's collected output as the next agent's input.

```rust
let pattern = SequentialPattern::new(vec![planner, coder, reviewer]);
let stream = pattern.run(input_messages, options).await?;
```

Flow:
```
[input] → Agent 1 → collect → Agent 2 → collect → ... → Agent N (streamed)
```

All but the last agent's output is collected via `collect_agent_response()` and piped as `ChatMessage::assistant(text)`.

### [`ConcurrentPattern`](src/patterns/concurrent.rs)

Fan-out/fan-in — runs all agents in parallel and merges their streams into one.

```rust
let pattern = ConcurrentPattern::new(vec![agent_a, agent_b, agent_c]);
let merged_stream = pattern.run(input_messages, options).await?;
```

Uses `futures_util::stream::select_all()` for stream merging.

### [`HandoffPattern`](src/patterns/handoff.rs)

Triage-based routing — one agent decides which target agent handles the request. Inspired by OpenAI Swarm's handoff pattern.

```rust
let pattern = HandoffPattern::new(vec![triage, specialist_a, specialist_b], 0);
let stream = pattern.run(input_messages, options).await?;
```

Current state: placeholder — triage agent is invoked but response parsing and automatic routing are under development. Can be used as a building block with manual routing via `find_agent()`.

## Usage

```rust
use rust_agent_workflow::{
    GraphFlow, SequentialPattern, ConcurrentPattern, HandoffPattern,
};
```

## Design Notes

- All components implement or compose `IAgent` — workflow outputs are themselves agent outputs
- Patterns are decoupled from `GraphFlow` — use them standalone or within a graph
- Stream-based design enables real-time output while agents execute
- Serialization-friendly data structures (via `serde`) enable checkpointing extensions

## Dependencies

- `rust-agent-core` — traits (`IAgent`, `ISession`) and types
- `rust-agent-framework` — agent implementations
- `futures-core`, `futures-util` — stream operations (`select_all`)
- `tokio` — async runtime
- `async-trait` — async trait support
- `serde`, `serde_json` — serialization
- `tracing` — instrumentation
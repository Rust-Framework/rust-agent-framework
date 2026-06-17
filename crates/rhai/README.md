# rust-agent-rhai

Rhai scripting engine integration for the Rust Agent Framework — dynamic embedded scripting for workflow nodes and agent tools.

## Overview

This crate bridges the [Rhai](https://rhai.rs/) embedded scripting language with RAF's workflow and tool systems:

- **`RhaiRuntime`** — Self-contained runtime managing the Rhai engine, scope, and module registration
- **`RhaiExecutor`** — Implements `IExecutor` trait, usable directly as a workflow node
- **`RhaiTool`** — Implements `ITool` trait, enabling agents to invoke Rhai scripts through the ToolRegistry

## Public API

### RhaiRuntime

Manages the Rhai scripting engine instance, scope, and registered modules.

```rust
use rust_agent_rhai::RhaiRuntime;

let runtime = RhaiRuntime::new();

// Register a custom function visible to scripts
runtime.engine_mut().register_fn("double", |x: i64| x * 2);

// Evaluate a script
let result: i64 = runtime.evaluate::<i64>("double(21)")?;
assert_eq!(result, 42);

// Evaluate with variables
runtime.scope_mut().push("name", "World");
let greeting: String = runtime.evaluate::<String>(r#""Hello, " + name + "!""#)?;
assert_eq!(greeting, "Hello, World!");
```

### RhaiExecutor

Implements `IExecutor` for use as a workflow graph node. Scripts can be inline strings or loaded from files.

```rust
use rust_agent_rhai::RhaiExecutor;
use rust_agent_workflow::{IExecutor, TypeTag, HandlerResult};
use std::sync::Arc;

let executor = Arc::new(RhaiExecutor::new(
    "transform-node",
    r#"
        let input = params["input"].as_string().unwrap();
        #{
            output: input.to_upper(),
            length: input.len(),
        }
    "#,
));

// Use in a workflow graph
// WorkflowBuilder::new()
//     .add_node("transform", executor)
//     .set_start("transform")
//     .build()?;
```

### RhaiTool

Implements `ITool` for agent-use. Pass a Rhai script path and JSON parameters schema.

```rust
use rust_agent_rhai::RhaiTool;

let tool = RhaiTool::new(
    "calculate",
    "Perform arithmetic calculations",
    serde_json::json!({
        "type": "object",
        "properties": {
            "expression": { "type": "string", "description": "Math expression" }
        },
        "required": ["expression"]
    }),
    "./tools/calc.rhai",
);

// Register with an agent
// AgentBuilder::new("math-agent")
//     .chat_client(client)
//     .with_tool(tool)
//     .build()?;
```

## Usage

```rust
use rust_agent_rhai::{RhaiRuntime, RhaiExecutor, RhaiTool};
```

## Dependencies

| Crate | Purpose |
|---|---|
| `rust-agent-core` | `ITool`, `AgentError` traits and types |
| `rust-agent-workflow` | `IExecutor` trait for workflow node integration |
| `rhai` (sync feature) | Rhai scripting engine |
| `parking_lot` | High-performance synchronization primitives |
| `tokio` | Async runtime |
| `serde` / `serde_json` | Script input/output serialization |
| `async-trait` | Async trait support |
| `tracing` | Structured logging |

# rust-agent-macros

Proc-macro helpers for the rust-agent-framework ecosystem. Provides the `#[tool]` attribute macro for ergonomic `ITool` definitions.

## The `#[tool]` Macro

A single-attribute macro that eliminates boilerplate when defining tools. Supports two patterns:

### Pattern 1: Async Function

```rust
use rust_agent_framework::tool;

#[tool(description = "Adds two numbers")]
async fn add(
    #[param(desc = "First number")] a: i64,
    #[param(desc = "Second number")] b: i64,
) -> String {
    format!("{}", a + b)
}

// Generates:
// - struct Add;                    (PascalCase of fn name)
// - struct AddArgs { a: i64, b: i64 }  (for deserialization)
// - impl ITool for Add { ... }     (with auto-generated JSON Schema)
```

What the macro generates:
- A unit struct with the PascalCase-ified function name (`Add`)
- An args struct for `serde_json::from_value` deserialization
- `ITool` implementation with:
  - `name()` → original function name (`"add"`)
  - `description()` → from `description = "..."` attribute
  - `parameters()` → auto-generated JSON Schema from parameter types and `#[param(desc = "...")]` annotations
  - `execute()` → deserializes args, calls `call()`, returns `Result<String>`
- A public `call()` method on the struct that delegates to the original function

Type mapping to JSON Schema:

| Rust Type | JSON Schema Type |
|---|---|
| `String`, `&str` | `"string"` |
| `i8`–`i128`, `u8`–`u128`, `isize`, `usize` | `"integer"` |
| `f32`, `f64` | `"number"` |
| `bool` | `"boolean"` |
| `Option<T>` | `T`'s schema (not required) |
| `Vec<T>` | `{"type": "array", "items": <T schema>}` |
| Other | `"string"` (fallback) |

### Pattern 2: Unit Struct

```rust
#[tool(description = "My custom tool")]
struct MyTool;

// You must implement call() manually:
impl MyTool {
    pub async fn call(&self, arguments: serde_json::Value) -> String {
        // custom logic
    }
}

// Macro generates impl ITool for MyTool { ... }
```

## Usage

The macro is re-exported from `rust-agent-framework`, so most users shouldn't depend on this crate directly:

```rust
use rust_agent_framework::tool;
```

If you only need the macro without the framework runtime:

```toml
[dependencies]
rust-agent-macros = "0.1"
```

## Implementation Details

The macro uses `syn` for full AST parsing and `quote` for token generation:

1. Parses the attribute for `description = "..."` (also accepts `desc = "..."`)
2. Parses the item as either `ItemFn` or `DeriveInput`
3. For functions: extracts parameter names, types, and `#[param(desc = "...")]` annotations
4. Generates JSON Schema property map per parameter
5. Generates `async fn call(&self, params) -> ReturnType` method
6. Generates `#[async_trait] impl ITool for Struct { ... }`

String escaping and whitespace handling in JSON values use `serde_json::Value::String()`, avoiding manual quote escaping.

## Dependencies

- `syn` (full, parsing features) — Rust syntax tree parsing
- `quote` — token generation
- `proc-macro2` — proc-macro token stream bridge
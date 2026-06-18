# rust-agent-macros

Proc-macro helpers for the rust-agent-framework ecosystem. Provides the `#[tool]` attribute macro for ergonomic `ITool` definitions.

## The `#[tool]` Macro

A single-attribute macro that eliminates boilerplate when defining tools. Supports three patterns:

### Pattern 1: Async Function (stateless tools)

```rust
use rust_agent_framework::tool;

#[tool(description = "Adds two numbers", kind = "function")]
async fn add(
    #[param(desc = "First number")] a: i64,
    #[param(desc = "Second number")] b: i64,
) -> rust_agent_core::ToolResult {
    rust_agent_core::ToolResult::success(serde_json::json!({"result": a + b}))
}
```

Generates:
- A unit struct `Add` (PascalCase of fn name)
- `AddArgs` struct for deserialization
- `impl ITool for Add` with auto-generated JSON Schema from parameter types and `#[param(desc)]` annotations

### Pattern 2: Impl Block (recommended for stateful tools)

```rust
pub struct ReadFile {
    pub scope: Option<Arc<WorkspaceScope>>,
}

#[tool(
    description = "Reads a file from the local filesystem.",
    kind = "file"
)]
impl ReadFile {
    async fn call(
        &self,
        #[param(desc = "Absolute path to the file")] path: String,
        #[param(desc = "Starting line number")] offset: Option<i64>,
    ) -> rust_agent_core::Result<ToolResult> {
        // Business logic uses typed parameters directly — no manual deserialization
    }
}
```

Generates:
- `ReadFileCallArgs` struct (hidden, auto-derives `Deserialize`)
- `impl ITool for ReadFile` with complete `parameters()` JSON Schema including parameter names, types, descriptions, and required fields
- Re-emits the original `impl ReadFile` block (with `#[param]` attrs stripped)

### Pattern 3: Struct (backward-compatible, legacy)

```rust
#[tool(description = "My custom tool", kind = "custom")]
struct MyTool;

impl MyTool {
    pub async fn call(&self, arguments: serde_json::Value) -> rust_agent_core::Result<ToolResult> {
        // Manual deserialization required; parameters() returns empty schema
    }
}
```

## `kind` Configuration

The `kind = "..."` attribute controls `ITool::kind()` output, mapping to `ToolDecl` categories:

| kind | Use case |
|------|----------|
| `"function"` | User-registered function tools (default) |
| `"custom"` | Factory-registered custom tools |
| `"web"` | Web search/fetch tools |
| `"file"` | File system tools |
| `"shell"` | Shell command execution |
| `"skills"` | Skill loading and resource tools |
| `"code"` | Code interpreter / sandbox |
| `"mcp"` | MCP remote tools |
| `"openapi"` | OpenAPI spec tools |

## Type Mapping to JSON Schema

| Rust Type | JSON Schema Type |
|---|---|
| `String`, `&str` | `"string"` |
| `i8`–`i128`, `u8`–`u128`, `isize`, `usize` | `"integer"` |
| `f32`, `f64` | `"number"` |
| `bool` | `"boolean"` |
| `Option<T>` | `T`'s schema (not required) |
| `Vec<T>` | `{"type": "array", "items": <T schema>}` |
| Other | `"string"` (fallback) |

## Usage

The macro is re-exported from `rust-agent-framework`:

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

1. Parses the attribute for `description = "..."` (also accepts `desc = "..."`) and `kind = "..."`.
2. Parses the item as `ItemFn`, `ItemImpl`, or `DeriveInput`.
3. For functions / impl blocks: extracts parameter names, types, and `#[param(desc = "...")]` annotations.
4. Generates JSON Schema property map per parameter.
5. Generates `async fn call(&self, params) -> ReturnType` method (fn mode) or preserves user-written call method (impl mode).
6. Generates `#[async_trait] impl ITool for Struct { ... }`.

## Dependencies

- `syn` (full, parsing features) — Rust syntax tree parsing
- `quote` — token generation
- `proc-macro2` — proc-macro token stream bridge

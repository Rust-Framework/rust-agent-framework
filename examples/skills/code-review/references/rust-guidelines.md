# Rust Coding Guidelines

## Naming

- Use `snake_case` for functions, variables, modules
- Use `CamelCase` for types, traits, enums
- Use `SCREAMING_SNAKE_CASE` for constants
- Avoid abbreviations unless widely known

## Error Handling

- Prefer `Result<T, E>` over panicking
- Use `thiserror` for library error types
- Use `anyhow` for application error handling
- Never silently ignore errors with `let _ = ...`

## Async

- Use `tokio` as the async runtime
- Avoid `block_on` in async contexts
- Prefer `Arc<dyn Trait>` over generic parameters for runtime polymorphism

## Testing

- Every public function should have at least one test
- Use `#[cfg(test)]` module
- Prefer integration tests for workflow validation

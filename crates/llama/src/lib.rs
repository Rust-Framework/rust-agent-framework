//! Local GGUF inference via [llama-gguf](https://crates.io/crates/llama-gguf).
//!
//! Provides [`LlamaChatClient`] — an [`IChatClient`](rust_agent_core::IChatClient) implementation
//! that loads and runs GGUF models locally.

mod chat_client;
mod engine;
mod options;
mod prompt;

pub use chat_client::{default_model_metadata, LlamaChatClient};
pub use llama_gguf::engine::ChatTemplate;
pub use options::LlamaChatClientOptions;
pub use prompt::{build_prompt, detect_prompt_style, PromptStyle};

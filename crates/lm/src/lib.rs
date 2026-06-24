//! Local LLM inference via [lm.rs](https://github.com/samuel-vitorino/lm.rs).
//!
//! Provides [`LmChatClient`] — an [`IChatClient`](rust_agent_core::IChatClient) implementation
//! that runs Gemma / Llama / Phi models on CPU from LMRS-format weight files.

mod chat_client;
mod engine;
mod options;
mod prompt;

pub use chat_client::{default_model_metadata, model_family_name, LmChatClient};
pub use options::LmChatClientOptions;

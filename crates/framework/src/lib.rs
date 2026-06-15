pub mod agent_host;
pub mod agent_runtime;
pub mod agents;
pub mod builder;
pub mod chat_client_agent;
pub mod chat_client_decorators;
pub mod compression;
pub mod context_providers;
pub mod converter;
pub mod session_store;
pub mod token_counter;
pub mod tools;

pub use agent_host::AgentHost;
pub use agent_runtime::AgentRuntime;
pub use builder::AgentBuilder;
pub use chat_client_agent::ChatClientAgent;
pub use context_providers::history_provider::InMemoryHistoryProvider;
pub use context_providers::memory_provider::{MemoryContextProvider, MemoryMode};
pub use chat_client_decorators::FunctionInvokingChatClient;
pub use chat_client_decorators::PerServiceCallPersistingChatClient;
pub use compression::SlidingWindowStrategy;
pub use compression::TokenBudgetStrategy;
pub use compression::CompressionPipeline;
pub use converter::AgentResponseConverter;
pub use session_store::InMemorySessionStore;
pub use session_store::FileSystemSessionStore;
pub use session_store::IsolationScopedSessionStore;
pub use session_store::IIsolationKeyProvider;
pub use session_store::FixedIsolationKeyProvider;
pub use token_counter::EstimateCounter;

// Re-export #[tool] macro — framework is the natural home for tool definition utilities
pub use rust_agent_macros::tool;

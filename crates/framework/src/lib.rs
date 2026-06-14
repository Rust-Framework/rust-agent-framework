pub mod agent_runtime;
pub mod agents;
pub mod builder;
pub mod chat_client_agent;
pub mod converter;
pub mod tools;

pub use agent_runtime::AgentRuntime;
pub use agents::tool_loop_agent::ToolLoopAgent;
pub use builder::AgentBuilder;
pub use chat_client_agent::ChatClientAgent;
pub use converter::AgentResponseConverter;

// Re-export #[tool] macro — framework is the natural home for tool definition utilities
pub use rust_agent_macros::tool;

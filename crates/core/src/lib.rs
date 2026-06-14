pub mod agent;
pub mod chat_client;
pub mod context_provider;
pub mod error;
pub mod incremental_json;
pub mod message;
pub mod run_options;
pub mod session;
pub mod stream;
pub mod tool;
pub mod types;

// Re-export interfaces
pub use agent::IAgent;
pub use chat_client::IChatClient;
pub use chat_client::ChatClientRunOptions;
pub use context_provider::{ContextInjection, IContextProvider};
pub use run_options::AgentRunOptions;
pub use run_options::ReasoningEffort;
pub use session::{AgentSession, ISession, ProviderStateStore, SessionMetadata, SessionSnapshot};
pub use stream::{BoxStream, collect_agent_response};
pub use tool::{ITool, ToolRegistry};

// Re-export core types
pub use error::{AgentError, Result};
pub use incremental_json::{ArgsEvent, StreamingArgsParser};
pub use message::{
    AgentResponse, AgentResponseResult, AgentResponseUpdate, ChatMessage, Content, CustomEvent,
    ErrorContent, Event, ExecutorInvokedEvent, ExecutorInvokingEvent, HasMeta, MessageRole,
    ReasoningContent, TextContent, ToolCallArgsContent, ToolCallArgsParsedContent,
    ToolCallArgsProgressContent, ToolCallEndContent, ToolCallStartContent,
    ToolCalledContent, ToolCallingContent, UriContent, UsageContent,
};
pub use types::{
    AgentId, AgentMetadata, FinishReason, ResponseMetadata, ToolCall, Usage,
};

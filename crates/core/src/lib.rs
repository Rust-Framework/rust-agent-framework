pub mod agent;
pub mod chat_client;
pub mod compression;
pub mod context_provider;
pub mod error;
pub mod incremental_json;
pub mod message;
pub mod model_metadata;
pub mod run_options;
pub mod session;
pub mod session_store;
pub mod stream;
pub mod token_counter;
pub mod tool;
pub mod types;
pub mod vector_store;

// Re-export interfaces
pub use agent::IAgent;
pub use chat_client::IChatClient;
pub use chat_client::ChatClientRunOptions;
pub use chat_client::ChatClientBuilder;
pub use chat_client::DelegatingChatClient;
pub use compression::ICompressionStrategy;
pub use context_provider::{ContextInjection, IContextProvider};
pub use model_metadata::ModelMetadata;
pub use run_options::AgentRunOptions;
pub use run_options::ReasoningEffort;
pub use session::{AgentSession, ISession, ProviderState, ProviderStateStore, SessionMetadata, SessionSnapshot, SessionTTLOptions};
pub use session_store::ISessionStore;
pub use stream::{BoxStream, collect_agent_response};
pub use token_counter::ITokenCounter;
pub use tool::{ITool, ToolRegistry};
pub use vector_store::{IVectorStore, SearchResult};

// Re-export core types
pub use error::{AgentError, Result};
pub use incremental_json::{ArgsEvent, StreamingArgsParser};
pub use message::{
    AgentResponse, AgentResponseResult, AgentResponseUpdate, ChatMessage, Content, CustomEvent,
    ErrorContent, Event, ExecutorInvokedEvent, ExecutorInvokingEvent, HasMeta, MessageRole,
    MessageSource, ReasoningContent, TextContent, ToolCallArgsContent, ToolCallArgsParsedContent,
    ToolCallArgsProgressContent, ToolCallEndContent, ToolCallStartContent,
    ToolCalledContent, ToolCallingContent, UriContent, UsageContent,
};
pub use types::{
    AgentId, AgentMetadata, FinishReason, ResponseMetadata, ToolCall, Usage,
};

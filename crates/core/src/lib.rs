pub mod agent;
pub mod chat_client;
pub mod error;
pub mod message;
pub mod middleware;
pub mod model;
pub mod prompt;
pub mod run_options;
pub mod session;
pub mod stream;
pub mod tool;
pub mod types;
pub mod workflow;

// Re-export interfaces
pub use agent::IAgent;
pub use chat_client::IChatClient;
pub use chat_client::ChatClientRunOptions;
pub use middleware::IMiddleware;
pub use model::ModelInfo;
pub use prompt::PromptTemplate;
pub use run_options::ChatAgentRunOptions;
pub use run_options::ReasoningEffort;
pub use session::{AgentSession, ISession};
pub use stream::{BoxStream, collect_agent_response};
pub use tool::{ITool, ToolRegistry};
pub use workflow::IWorkflow;

// Re-export core types
pub use error::{AgentError, Result};
pub use message::{AgentResponse, AgentStreamChunk, ChatMessage, ChatStreamChunk, MessageRole};
pub use types::{AgentId, AgentMetadata, ToolCall, ToolCallDelta, ToolResult};

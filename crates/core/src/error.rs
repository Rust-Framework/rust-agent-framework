use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Chat client error: {0}")]
    ChatClientError(String),

    #[error("Tool execution error: {0}")]
    ToolError(String),

    #[error("Workflow error: {0}")]
    WorkflowError(String),

    #[error("Session error: {0}")]
    SessionError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Stream error: {0}")]
    StreamError(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, AgentError>;

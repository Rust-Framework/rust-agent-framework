use thiserror::Error;

/// Errors that can occur during declaration parsing, validation, or resolution.
#[derive(Debug, Error)]
pub enum DeclError {
    /// I/O error while reading a declaration file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parsing error.
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML parsing error.
    #[cfg(feature = "yaml")]
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// TOML parsing error.
    #[cfg(feature = "toml")]
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// TOML serialization error.
    #[cfg(feature = "toml")]
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    /// Resolution failure — the declaration is valid but cannot be built.
    #[error("Resolution error: {0}")]
    Resolution(String),

    /// Unsupported configuration in the declaration.
    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// Validation error in the declaration data.
    #[error("Validation error: {0}")]
    Validation(String),

    /// Wraps a core error.
    #[error("Agent error: {0}")]
    Agent(#[from] rust_agent_core::AgentError),

    /// Required field or reference is missing.
    #[error("Missing: {0}")]
    Missing(String),

    /// Expression evaluation error (PowerFx or template).
    #[error("Expression error: {0}")]
    Expression(String),
}

/// Convenience alias for results in this crate.
pub type Result<T> = std::result::Result<T, DeclError>;

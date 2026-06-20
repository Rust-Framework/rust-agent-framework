use thiserror::Error;

/// 声明解析、验证或解析期间可能发生的错误。
#[derive(Debug, Error)]
pub enum DeclError {
    /// 读取声明文件时的 I/O 错误。
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 解析错误。
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML 解析错误。
    #[cfg(feature = "yaml")]
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// TOML 解析错误。
    #[cfg(feature = "toml")]
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// TOML 序列化错误。
    #[cfg(feature = "toml")]
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    /// 解析失败——声明有效但无法构建。
    #[error("Resolution error: {0}")]
    Resolution(String),

    /// 声明中不支持的配置。
    #[error("Unsupported: {0}")]
    Unsupported(String),

    /// 声明数据中的验证错误。
    #[error("Validation error: {0}")]
    Validation(String),

    /// 包装核心错误。
    #[error("Agent error: {0}")]
    Agent(#[from] rust_agent_core::AgentError),

    /// 必需的字段或引用缺失。
    #[error("Missing: {0}")]
    Missing(String),

    /// 表达式求值错误（Rhai 或模板）。
    #[error("Expression error: {0}")]
    Expression(String),
}

/// 此 crate 中结果的便捷别名。
pub type Result<T> = std::result::Result<T, DeclError>;

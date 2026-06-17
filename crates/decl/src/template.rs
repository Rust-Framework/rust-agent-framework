use serde::{Deserialize, Serialize};

/// Template configuration for prompt rendering.
/// Aligns with MAF AgentSchema v1.0 `Template`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// The template rendering engine format.
    pub format: TemplateFormat,
    /// The parser used to process rendered templates.
    pub parser: TemplateParser,
}

/// Template rendering engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateFormat {
    /// Mustache template engine (used by MAF).
    Mustache,
    /// Jinja2 template engine.
    Jinja2,
    /// Custom/unknown format (captures as string).
    #[serde(untagged)]
    Custom(String),
}

/// Parser for processing rendered templates into API-compatible format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateParser {
    /// Prompty parser (Microsoft's prompt template parser).
    Prompty,
    /// Custom/unknown parser (captures as string).
    #[serde(untagged)]
    Custom(String),
}

impl Template {
    /// Create a new template with mustache format and prompty parser (MAF defaults).
    pub fn mustache_prompty() -> Self {
        Self {
            format: TemplateFormat::Mustache,
            parser: TemplateParser::Prompty,
        }
    }

    /// Check if the template uses mustache format.
    pub fn is_mustache(&self) -> bool {
        matches!(self.format, TemplateFormat::Mustache)
    }

    /// Check if the template uses prompty parser.
    pub fn is_prompty(&self) -> bool {
        matches!(self.parser, TemplateParser::Prompty)
    }
}

use serde::{Deserialize, Serialize};

/// 提示词渲染的模板配置，与 MAF AgentSchema v1.0 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// 模板渲染引擎格式。
    pub format: TemplateFormat,
    /// 用于处理渲染后模板的解析器。
    pub parser: TemplateParser,
}

/// 模板渲染引擎。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateFormat {
    /// Mustache 模板引擎（MAF 使用）。
    Mustache,
    /// Jinja2 模板引擎。
    Jinja2,
    /// 自定义/未知格式（捕获为字符串）。
    #[serde(untagged)]
    Custom(String),
}

/// 将渲染后的模板处理为 API 兼容格式的解析器。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateParser {
    /// Prompty 解析器（Microsoft 的提示模板解析器）。
    Prompty,
    /// 自定义/未知解析器（捕获为字符串）。
    #[serde(untagged)]
    Custom(String),
}

impl Template {
    /// 使用 mustache 格式和 prompty 解析器创建新模板（MAF 默认值）。
    pub fn mustache_prompty() -> Self {
        Self {
            format: TemplateFormat::Mustache,
            parser: TemplateParser::Prompty,
        }
    }

    /// 检查模板是否使用 mustache 格式。
    pub fn is_mustache(&self) -> bool {
        matches!(self.format, TemplateFormat::Mustache)
    }

    /// 检查模板是否使用 prompty 解析器。
    pub fn is_prompty(&self) -> bool {
        matches!(self.parser, TemplateParser::Prompty)
    }
}

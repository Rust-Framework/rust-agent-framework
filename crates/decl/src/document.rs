use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::definition::AgentDefinition;
use crate::error::Result;
use crate::schema::PropertySchema;

/// 声明式 Agent 文件的顶层文档类型。
///
/// 可接受完整的 `AgentManifest`（部署包）或裸 `AgentDefinition`（内联定义），
/// 兼容 MAF 的使用模式——部分 YAML 文件包含 manifest 包装器，其余为裸定义。
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum AgentDocument {
    /// 完整部署 manifest，含模板和参数。
    Manifest(AgentManifest),
    /// 裸 Agent 定义，无 manifest 包装。
    Definition(AgentDefinition),
}

impl<'de> Deserialize<'de> for AgentDocument {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Parse as a raw JSON Value first
        let value = serde_json::Value::deserialize(deserializer)?;

        // Try Manifest first (requires `template` field with nested agent definition)
        if value.get("template").is_some() {
            if let Ok(manifest) = AgentManifest::deserialize(value.clone()) {
                return Ok(AgentDocument::Manifest(manifest));
            }
        }

        // Fall back to Definition (requires `kind` field)
        AgentDefinition::deserialize(value)
            .map(AgentDocument::Definition)
            .map_err(serde::de::Error::custom)
    }
}

/// 用于动态创建 Agent 的部署 Manifest，与 MAF AgentSchema v1.0 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    /// Manifest 名称。
    pub name: String,
    /// 人类可读的展示名称。
    #[serde(default, rename = "displayName")]
    pub display_name: String,
    /// Agent 能力与用途的描述。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// 附加元数据（作者、标签等）。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// 核心 Agent 模板定义。
    pub template: AgentDefinition,
    /// 部署时配置 Agent 的参数。
    pub parameters: PropertySchema,
    /// 必需的资源（模型、工具、连接）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ManifestResource>,
}

/// Agent Manifest 中的资源声明。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestResource {
    /// 资源名称。
    pub name: String,
    /// 资源类型（例如 "model"、"tool"、"connection"）。
    pub kind: String,
    /// 可选的资源标识符。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 附加的资源特有字段。
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl AgentDocument {
    // ── JSON ──

    /// 从 JSON 字符串解析 `AgentDocument`。
    pub fn from_json_str(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// 从 JSON 文件加载 `AgentDocument`。
    pub fn from_json_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json_str(&content)
    }

    /// 序列化为 JSON 字符串。
    pub fn to_json_string(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// 序列化为美化打印的 JSON 字符串。
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    // ── YAML ──

    /// 从 YAML 字符串解析 `AgentDocument`。
    #[cfg(feature = "yaml")]
    pub fn from_yaml_str(s: &str) -> Result<Self> {
        Ok(serde_yaml::from_str(s)?)
    }

    /// 从 YAML 文件加载 `AgentDocument`。
    #[cfg(feature = "yaml")]
    pub fn from_yaml_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml_str(&content)
    }

    /// 序列化为 YAML 字符串。
    #[cfg(feature = "yaml")]
    pub fn to_yaml_string(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }

    // ── TOML ──

    /// 从 TOML 字符串解析 `AgentDocument`。
    #[cfg(feature = "toml")]
    pub fn from_toml_str(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }

    /// 从 TOML 文件加载 `AgentDocument`。
    #[cfg(feature = "toml")]
    pub fn from_toml_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    /// 序列化为 TOML 字符串。
    #[cfg(feature = "toml")]
    pub fn to_toml_string(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    // ── Conversion helpers ──

    /// 解包为 Manifest，消费自身。
    pub fn into_manifest(self) -> Option<AgentManifest> {
        match self {
            AgentDocument::Manifest(m) => Some(m),
            AgentDocument::Definition(_) => None,
        }
    }

    /// 解包为 Definition，消费自身。
    pub fn into_definition(self) -> Option<AgentDefinition> {
        match self {
            AgentDocument::Manifest(_) => None,
            AgentDocument::Definition(d) => Some(d),
        }
    }

    /// 尝试提取内部的 `AgentDefinition`，无论是否有包装。
    pub fn inner_definition(&self) -> &AgentDefinition {
        match self {
            AgentDocument::Manifest(m) => &m.template,
            AgentDocument::Definition(d) => d,
        }
    }
}

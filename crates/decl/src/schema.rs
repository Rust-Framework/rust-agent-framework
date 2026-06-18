use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 用于 Agent 输入/输出的属性模式定义，与 MAF AgentSchema v1.0 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertySchema {
    pub properties: Vec<Property>,
    #[serde(default)]
    pub strict: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<HashMap<String, serde_json::Value>>,
}

/// `PropertySchema` 中的单个属性。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub kind: PropertyType,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "enumValues")]
    pub enum_values: Vec<serde_json::Value>,
}

/// MAF AgentSchema 中支持的属性数据类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PropertyType {
    String,
    Integer,
    Float,
    Boolean,
    Array,
    Object,
}

impl PropertySchema {
    /// 创建一个空的属性模式（无属性，非严格模式）。
    pub fn empty() -> Self {
        Self {
            properties: Vec::new(),
            strict: false,
            examples: Vec::new(),
        }
    }

    /// 用给定属性创建模式。
    pub fn new(properties: Vec<Property>) -> Self {
        Self {
            properties,
            strict: false,
            examples: Vec::new(),
        }
    }

    /// 按名称查找属性。
    pub fn find_property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }
}

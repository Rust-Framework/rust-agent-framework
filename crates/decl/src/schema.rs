use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Property schema definition for input/output of agents.
/// Aligns with MAF AgentSchema v1.0 `PropertySchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertySchema {
    pub properties: Vec<Property>,
    #[serde(default)]
    pub strict: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<HashMap<String, serde_json::Value>>,
}

/// A single property within a `PropertySchema`.
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

/// Supported property data types in MAF AgentSchema.
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
    /// Create an empty property schema (no properties, not strict).
    pub fn empty() -> Self {
        Self {
            properties: Vec::new(),
            strict: false,
            examples: Vec::new(),
        }
    }

    /// Create a schema with the given properties.
    pub fn new(properties: Vec<Property>) -> Self {
        Self {
            properties,
            strict: false,
            examples: Vec::new(),
        }
    }

    /// Find a property by name.
    pub fn find_property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }
}

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::definition::AgentDefinition;
use crate::error::Result;
use crate::schema::PropertySchema;

/// Top-level document type for declarative agent files.
///
/// Accepts either a full `AgentManifest` (deployment package) or a
/// raw `AgentDefinition` (inline definition), matching MAF usage patterns
/// where some YAML files contain manifest wrappers and others are bare definitions.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum AgentDocument {
    /// Full deployment manifest with template and parameters.
    Manifest(AgentManifest),
    /// Bare agent definition (no manifest wrapper).
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

/// Deployment manifest for creating agents dynamically.
/// Aligns with MAF AgentSchema v1.0 `AgentManifest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    /// Name of the manifest.
    pub name: String,
    /// Human-readable display name.
    #[serde(default, rename = "displayName")]
    pub display_name: String,
    /// Description of the agent's capabilities and purpose.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Additional metadata (authors, tags, etc.).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// The core agent template definition.
    pub template: AgentDefinition,
    /// Parameters for configuring the agent at deployment time.
    pub parameters: PropertySchema,
    /// Required resources (models, tools, connections).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ManifestResource>,
}

/// A resource declaration in an agent manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestResource {
    /// Resource name.
    pub name: String,
    /// Resource kind (e.g., "model", "tool", "connection").
    pub kind: String,
    /// Optional resource identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Additional resource-specific fields.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl AgentDocument {
    // ── JSON ──

    /// Parse an `AgentDocument` from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Load an `AgentDocument` from a JSON file.
    pub fn from_json_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_json_str(&content)
    }

    /// Serialize to a JSON string.
    pub fn to_json_string(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    /// Serialize to a pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    // ── YAML ──

    /// Parse an `AgentDocument` from a YAML string.
    #[cfg(feature = "yaml")]
    pub fn from_yaml_str(s: &str) -> Result<Self> {
        Ok(serde_yaml::from_str(s)?)
    }

    /// Load an `AgentDocument` from a YAML file.
    #[cfg(feature = "yaml")]
    pub fn from_yaml_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml_str(&content)
    }

    /// Serialize to a YAML string.
    #[cfg(feature = "yaml")]
    pub fn to_yaml_string(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }

    // ── TOML ──

    /// Parse an `AgentDocument` from a TOML string.
    #[cfg(feature = "toml")]
    pub fn from_toml_str(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }

    /// Load an `AgentDocument` from a TOML file.
    #[cfg(feature = "toml")]
    pub fn from_toml_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml_str(&content)
    }

    /// Serialize to a TOML string.
    #[cfg(feature = "toml")]
    pub fn to_toml_string(&self) -> Result<String> {
        Ok(toml::to_string(self)?)
    }

    // ── Conversion helpers ──

    /// Unwrap as a Manifest, consuming self.
    pub fn into_manifest(self) -> Option<AgentManifest> {
        match self {
            AgentDocument::Manifest(m) => Some(m),
            AgentDocument::Definition(_) => None,
        }
    }

    /// Unwrap as a Definition, consuming self.
    pub fn into_definition(self) -> Option<AgentDefinition> {
        match self {
            AgentDocument::Manifest(_) => None,
            AgentDocument::Definition(d) => Some(d),
        }
    }

    /// Try to extract the inner `AgentDefinition` regardless of wrapping.
    pub fn inner_definition(&self) -> &AgentDefinition {
        match self {
            AgentDocument::Manifest(m) => &m.template,
            AgentDocument::Definition(d) => d,
        }
    }
}

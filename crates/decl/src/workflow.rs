use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agent::AgentDecl;
use crate::error::Result;

// ── Node Declaration ──

/// A node in a workflow graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeDecl {
    /// An agent node — wraps an existing agent as a workflow executor.
    Agent {
        /// Unique node identifier within the workflow.
        id: String,
        /// Reference to a registered agent by its `AgentDecl.id`.
        agent_ref: String,
        /// Optional inline agent declaration (takes precedence over `agent_ref`).
        #[serde(default)]
        agent: Option<AgentDecl>,
        /// Mark this node as a workflow output.
        #[serde(default)]
        is_output: bool,
    },
    /// A pure-function node registered via factory.
    Function {
        /// Unique node identifier.
        id: String,
        /// Factory registration name for the function.
        function_ref: String,
        /// Arbitrary configuration forwarded to the factory.
        #[serde(default)]
        config: HashMap<String, serde_json::Value>,
        /// Mark this node as a workflow output.
        #[serde(default)]
        is_output: bool,
    },
    /// A Rhai script node.
    Rhai {
        /// Unique node identifier.
        id: String,
        /// Path to the Rhai script file.
        script_path: String,
        /// Mark this node as a workflow output.
        #[serde(default)]
        is_output: bool,
    },
}

// ── Edge Declaration ──

/// An edge connecting nodes in a workflow graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeDecl {
    /// Direct edge: `source` -> `target`.
    Direct {
        source: String,
        target: String,
    },
    /// Fan-out edge: `source` -> all `targets` in parallel.
    FanOut {
        source: String,
        targets: Vec<String>,
    },
    /// Fan-in edge: all `sources` must complete before `target`.
    FanIn {
        sources: Vec<String>,
        target: String,
    },
}

// ── Port Declaration ──

/// An external request port on the workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortDecl {
    /// Port identifier.
    pub id: String,
    /// Description of the port's purpose.
    #[serde(default)]
    pub description: String,
}

// ── Workflow Declaration ──

/// Complete declarative definition of a workflow graph.
///
/// Follows the **Agent Declaration Protocol** for multi-agent orchestration.
/// Models a directed graph of agent nodes connected by typed edges (direct,
/// fan-out, fan-in), compatible with standard workflow execution patterns.
///
/// Mirrors every capability of `WorkflowBuilder`, expressed as serializable data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDecl {
    /// Protocol version. Current version: `"1.0"`.
    #[serde(default = "default_protocol_version")]
    pub version: String,
    /// URI of the JSON Schema for this declaration format.
    #[serde(default, rename = "$schema", skip_serializing_if = "String::is_empty")]
    pub schema: String,
    /// Human-readable workflow name.
    pub name: String,
    /// All nodes in the graph.
    pub nodes: Vec<NodeDecl>,
    /// All edges connecting the nodes.
    #[serde(default)]
    pub edges: Vec<EdgeDecl>,
    /// Entry-point node ID.
    pub start_node_id: String,
    /// Node IDs whose outputs are exposed as workflow outputs.
    #[serde(default)]
    pub output_node_ids: Vec<String>,
    /// External request ports.
    #[serde(default)]
    pub ports: Vec<PortDecl>,
}

fn default_protocol_version() -> String {
    "1.0".into()
}

impl WorkflowDecl {
    // ── JSON ──

    /// Parse a `WorkflowDecl` from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Load a `WorkflowDecl` from a JSON file.
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

    /// Parse a `WorkflowDecl` from a YAML string.
    #[cfg(feature = "yaml")]
    pub fn from_yaml_str(s: &str) -> Result<Self> {
        Ok(serde_yaml::from_str(s)?)
    }

    /// Load a `WorkflowDecl` from a YAML file.
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

    /// Parse a `WorkflowDecl` from a TOML string.
    #[cfg(feature = "toml")]
    pub fn from_toml_str(s: &str) -> Result<Self> {
        Ok(toml::from_str(s)?)
    }

    /// Load a `WorkflowDecl` from a TOML file.
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
}

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::container_agent::ContainerAgentData;
use crate::prompt_agent::PromptAgentData;
use crate::schema::PropertySchema;
use crate::workflow_decl::WorkflowAgentData;

/// The unified agent definition type.
/// Aligns with MAF AgentSchema v1.0 `AgentDefinition`.
///
/// This struct holds the common base fields shared by all agent kinds
/// (name, description, metadata, input/output schema), and delegates
/// kind-specific data to `AgentKindData` via serde flatten.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Human-readable name of the agent.
    pub name: String,
    /// Display name for UI purposes.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "displayName")]
    pub display_name: Option<String>,
    /// Description of the agent's capabilities and purpose.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Additional metadata including authors, tags, etc.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Input parameters that participate in template rendering.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "inputSchema")]
    pub input_schema: Option<PropertySchema>,
    /// Expected output format and structure from the agent.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "outputSchema")]
    pub output_schema: Option<PropertySchema>,

    /// Kind-specific data (prompt, hosted, workflow).
    /// The `kind` field is injected by the internally-tagged enum below.
    #[serde(flatten)]
    pub kind_data: AgentKindData,
}

/// Kind-specific agent data, discriminated by the `kind` field.
/// Aligns with MAF `kind: "prompt"`, `kind: "hosted"`, `kind: "workflow"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentKindData {
    /// Prompt-based AI agent.
    #[serde(rename = "prompt")]
    Prompt(PromptAgentData),
    /// Hosted/container-based agent.
    #[serde(rename = "hosted")]
    Container(ContainerAgentData),
    /// Workflow orchestration agent.
    #[serde(rename = "workflow")]
    Workflow(WorkflowAgentData),
}

impl AgentDefinition {
    /// Create a new prompt agent definition.
    pub fn new_prompt(name: impl Into<String>, model: crate::model::Model) -> Self {
        Self {
            name: name.into(),
            display_name: None,
            description: String::new(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            kind_data: AgentKindData::Prompt(PromptAgentData::new(model)),
        }
    }

    /// Create a new workflow definition.
    pub fn new_workflow(name: impl Into<String>, trigger_kind: impl Into<String>, trigger_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            display_name: None,
            description: String::new(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            kind_data: AgentKindData::Workflow(WorkflowAgentData::new(trigger_kind, trigger_id)),
        }
    }

    /// Create a new container/hosted agent definition.
    pub fn new_container(name: impl Into<String>, resources: crate::container_agent::ContainerResources) -> Self {
        use crate::container_agent::{ContainerAgentData, ProtocolVersionRecord};
        Self {
            name: name.into(),
            display_name: None,
            description: String::new(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            kind_data: AgentKindData::Container(ContainerAgentData::new(
                vec![ProtocolVersionRecord::new("responses")],
                resources,
            )),
        }
    }

    /// Check if this is a prompt agent.
    pub fn is_prompt(&self) -> bool {
        matches!(self.kind_data, AgentKindData::Prompt(_))
    }

    /// Check if this is a workflow agent.
    pub fn is_workflow(&self) -> bool {
        matches!(self.kind_data, AgentKindData::Workflow(_))
    }

    /// Check if this is a container agent.
    pub fn is_container(&self) -> bool {
        matches!(self.kind_data, AgentKindData::Container(_))
    }
}

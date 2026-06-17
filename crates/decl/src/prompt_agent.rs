use serde::{Deserialize, Serialize};

use crate::definition::AgentDefinition;
use crate::model::Model;
use crate::template::Template;
use crate::tools::ToolDecl;

fn default_max_tool_rounds() -> usize {
    10
}

/// Prompt-based agent data (kind = "prompt").
/// Aligns with MAF AgentSchema v1.0 `PromptAgent`.
///
/// This is the most common agent type, supporting model configuration,
/// tool registration, template-based prompt rendering, and instructions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAgentData {
    /// Primary AI model configuration (required in MAF).
    pub model: Model,

    /// Tools available to the agent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDecl>,

    /// Template configuration for prompt rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,

    /// System instructions / prompt for the agent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instructions: String,

    /// Additional instructions or context for the agent.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "additionalInstructions")]
    pub additional_instructions: Option<String>,

    // ── Extension fields (non-MAF, framework-specific) ──

    /// Maximum tool-calling rounds before forced stop.
    #[serde(default = "default_max_tool_rounds", skip_serializing_if = "is_default_max_tool_rounds", rename = "maxToolRounds")]
    pub max_tool_rounds: usize,

    /// Nested sub-agent declarations (recursive `AgentDefinition` entries).
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "subAgents")]
    pub sub_agents: Vec<AgentDefinition>,
}

fn is_default_max_tool_rounds(v: &usize) -> bool {
    *v == default_max_tool_rounds()
}

impl PromptAgentData {
    /// Create a prompt agent with the given model.
    pub fn new(model: Model) -> Self {
        Self {
            model,
            tools: Vec::new(),
            template: None,
            instructions: String::new(),
            additional_instructions: None,
            max_tool_rounds: default_max_tool_rounds(),
            sub_agents: Vec::new(),
        }
    }

    /// Add a tool to the agent.
    pub fn with_tool(mut self, tool: ToolDecl) -> Self {
        self.tools.push(tool);
        self
    }

    /// Set the system instructions.
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    /// Add additional instructions.
    pub fn with_additional_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.additional_instructions = Some(instructions.into());
        self
    }

    /// Set the template configuration.
    pub fn with_template(mut self, template: Template) -> Self {
        self.template = Some(template);
        self
    }

    /// Set the maximum tool-calling rounds.
    pub fn with_max_tool_rounds(mut self, rounds: usize) -> Self {
        self.max_tool_rounds = rounds;
        self
    }

    /// Add a sub-agent.
    pub fn with_sub_agent(mut self, sub_agent: AgentDefinition) -> Self {
        self.sub_agents.push(sub_agent);
        self
    }
}

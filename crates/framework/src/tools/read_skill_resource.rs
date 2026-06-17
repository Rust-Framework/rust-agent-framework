use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use crate::context_providers::agent_skill::AgentSkill;

/// Read a resource file from a loaded skill's references/ or assets/ directory.
///
/// Path resolution and traversal protection are delegated to
/// `AgentSkill::read_resource()`.
pub struct ReadSkillResourceTool {
    skills: Arc<Vec<AgentSkill>>,
}

impl ReadSkillResourceTool {
    pub fn new(skills: Arc<Vec<AgentSkill>>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl ITool for ReadSkillResourceTool {
    fn name(&self) -> &str {
        "read_skill_resource"
    }

    fn description(&self) -> &str {
        "Read a resource file from a loaded skill's references/ or assets/ directory. Use this to access supplementary documents, templates, or style guides."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The name of the skill"
                },
                "resource_path": {
                    "type": "string",
                    "description": "Relative path to the resource file (e.g., 'references/style-guide.md')"
                }
            },
            "required": ["skill_name", "resource_path"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        let skill_name = arguments["skill_name"]
            .as_str()
            .ok_or_else(|| {
                rust_agent_core::AgentError::ToolError(
                    "Missing 'skill_name' argument".into(),
                )
            })?;
        let resource_path = arguments["resource_path"]
            .as_str()
            .ok_or_else(|| {
                rust_agent_core::AgentError::ToolError(
                    "Missing 'resource_path' argument".into(),
                )
            })?;

        let skill = self.skills.iter().find(|s| s.metadata.name == skill_name).ok_or_else(|| {
            rust_agent_core::AgentError::ToolError(format!(
                "Skill '{}' not found",
                skill_name
            ))
        })?;

        // AgentSkill::read_resource() handles path resolution and traversal protection
        let content = skill.read_resource(resource_path)?;
        Ok(serde_json::json!({
            "ok": true,
            "data": {
                "skill_name": skill_name,
                "resource_path": resource_path,
                "content": content,
            }
        }).to_string())
    }
}

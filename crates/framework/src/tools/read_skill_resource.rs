use std::sync::Arc;

use rust_agent_core::ToolResult;
use rust_agent_macros::tool;

use crate::context_providers::agent_skill::AgentSkill;

#[tool(description = "Read a resource file from a loaded skill's references/ or assets/ directory. Use this to access supplementary documents, templates, or style guides.")]
pub struct ReadSkillResourceTool {
    pub skills: Arc<Vec<AgentSkill>>,
}

impl ReadSkillResourceTool {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        let skill_name = arguments["skill_name"].as_str().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError("Missing 'skill_name' argument".into())
        })?;
        let resource_path = arguments["resource_path"].as_str().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError("Missing 'resource_path' argument".into())
        })?;

        let skill = self
            .skills
            .iter()
            .find(|s| s.metadata.name == skill_name)
            .ok_or_else(|| {
                rust_agent_core::AgentError::ToolError(format!("Skill '{}' not found", skill_name))
            })?;

        let content = skill.read_resource(resource_path).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Failed to read resource '{}' from skill '{}': {}",
                resource_path, skill_name, e
            ))
        })?;

        Ok(ToolResult::success(serde_json::json!({
            "skill_name": skill_name,
            "resource_path": resource_path,
            "content": content,
        })))
    }
}

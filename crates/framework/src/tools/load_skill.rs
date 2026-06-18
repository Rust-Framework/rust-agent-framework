use std::sync::Arc;

use rust_agent_core::{ToolResult};
use rust_agent_macros::tool;

use crate::context_providers::agent_skill::AgentSkill;

#[tool(description = "Load a skill's full instructions from SKILL.md. Call this when a user's task matches a skill's domain to get detailed step-by-step guidance.")]
pub struct LoadSkillTool {
    pub skills: Arc<Vec<AgentSkill>>,
}

impl LoadSkillTool {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        let skill_name = arguments["skill_name"].as_str().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError("Missing 'skill_name' argument".into())
        })?;

        let skill = self
            .skills
            .iter()
            .find(|s| s.metadata.name == skill_name)
            .ok_or_else(|| {
                rust_agent_core::AgentError::ToolError(format!(
                    "Skill '{}' not found. Available skills: {}",
                    skill_name,
                    self.skills
                        .iter()
                        .map(|s| s.metadata.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;

        let instructions = skill.load_instructions()?;
        Ok(ToolResult::success(serde_json::json!({
            "skill_name": skill_name,
            "instructions": instructions,
        })))
    }
}

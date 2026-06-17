use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use crate::context_providers::agent_skill::AgentSkill;

/// Load a skill's full instructions from SKILL.md.
///
/// This tool exposes the `load_skill` operation as a standalone `ITool`,
/// replacing the inline `FnTool` previously defined in `AgentSkillsProvider`.
pub struct LoadSkillTool {
    skills: Arc<Vec<AgentSkill>>,
}

impl LoadSkillTool {
    pub fn new(skills: Arc<Vec<AgentSkill>>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl ITool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn description(&self) -> &str {
        "Load a skill's full instructions from SKILL.md. Call this when a user's task matches a skill's domain to get detailed step-by-step guidance."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The name of the skill to load"
                }
            },
            "required": ["skill_name"]
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

        let skill = self.skills.iter().find(|s| s.metadata.name == skill_name).ok_or_else(|| {
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
        Ok(serde_json::json!({
            "ok": true,
            "data": {
                "skill_name": skill_name,
                "instructions": instructions,
            }
        }).to_string())
    }
}

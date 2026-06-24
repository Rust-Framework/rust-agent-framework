use std::sync::Arc;

use rust_agent_core::ToolResult;
use rust_agent_macros::tool;

use crate::context::skill::AgentSkill;

pub struct LoadSkillTool {
    pub skills: Arc<Vec<AgentSkill>>,
}

#[tool(
    description = "加载 SKILL.md 中的技能完整指引。当用户任务匹配某个技能领域时，调用此工具获取详细的分步操作指导。",
    kind = "skills"
)]
impl LoadSkillTool {
    async fn call(
        &self,
        #[param(desc = "要加载的技能名称")] skill_name: String,
    ) -> rust_agent_core::Result<ToolResult> {
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

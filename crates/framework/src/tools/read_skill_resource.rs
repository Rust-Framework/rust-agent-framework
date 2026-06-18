use std::sync::Arc;

use rust_agent_core::ToolResult;
use rust_agent_macros::tool;

use crate::context_providers::agent_skill::AgentSkill;

pub struct ReadSkillResourceTool {
    pub skills: Arc<Vec<AgentSkill>>,
}

#[tool(
    description = "读取已加载技能中 references/ 或 assets/ 目录下的资源文件。用于访问补充文档、模板或样式指南。",
    kind = "skills"
)]
impl ReadSkillResourceTool {
    async fn call(
        &self,
        #[param(desc = "技能名称")] skill_name: String,
        #[param(desc = "资源文件在技能目录内的相对路径")] resource_path: String,
    ) -> rust_agent_core::Result<ToolResult> {
        let skill = self
            .skills
            .iter()
            .find(|s| s.metadata.name == skill_name)
            .ok_or_else(|| {
                rust_agent_core::AgentError::ToolError(format!("Skill '{}' not found", skill_name))
            })?;

        let content = skill.read_resource(&resource_path).map_err(|e| {
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

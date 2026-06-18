use std::sync::Arc;

use rust_agent_core::ToolResult;
use rust_agent_macros::tool;

use crate::context_providers::agent_skill::AgentSkill;
use crate::context_providers::script_runner::AgentSkillScriptRunner;

#[tool(description = "Execute a script from a skill's scripts/ directory. Use this to run validation, analysis, or automation scripts bundled with a skill.")]
pub struct RunSkillScriptTool {
    pub skills: Arc<Vec<AgentSkill>>,
    pub runner: Option<Arc<dyn AgentSkillScriptRunner>>,
}

impl RunSkillScriptTool {
    async fn call(
        &self,
        arguments: serde_json::Value,
    ) -> rust_agent_core::Result<ToolResult> {
        let skill_name = arguments["skill_name"].as_str().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError("Missing 'skill_name' argument".into())
        })?;
        let script_path = arguments["script_path"].as_str().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError("Missing 'script_path' argument".into())
        })?;

        let args: Vec<String> = arguments["args"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let timeout_secs = arguments["timeout_secs"].as_u64();

        let skill = self
            .skills
            .iter()
            .find(|s| s.metadata.name == skill_name)
            .ok_or_else(|| {
                rust_agent_core::AgentError::ToolError(format!("Skill '{}' not found", skill_name))
            })?;

        let runner = self.runner.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError(
                "No script runner configured".into(),
            )
        })?;

        // Skill's scripts are in `<skill_root>/scripts/`
        let skill_dir = skill.root_dir.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError(
                "Skill has no root directory (dynamically created skills don't support scripts)".into(),
            )
        })?;
        let full_path = skill_dir.join("scripts").join(script_path);

        // Path traversal guard: canonicalize and verify it's under skill dir
        let canonical = full_path.canonicalize().map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Script path resolution failed: {}",
                e
            ))
        })?;
        let canonical_root = skill_dir.canonicalize().unwrap_or_else(|_| skill_dir.clone());
        if !canonical.starts_with(&canonical_root) {
            return Ok(ToolResult::error("Script path traversal denied"));
        }

        match runner
            .run(skill_name, &canonical, Some(args), timeout_secs)
            .await {
            Ok(output) => Ok(ToolResult::success(serde_json::json!({
                "skill_name": skill_name,
                "script_path": script_path,
                "output": output,
            }))),
            Err(e) => Ok(ToolResult::error(format!("Script execution failed: {}", e))),
        }
    }
}

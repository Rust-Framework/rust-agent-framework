use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{ITool, Result};

use crate::context_providers::agent_skill::AgentSkill;
use crate::context_providers::script_runner::AgentSkillScriptRunner;

/// 从技能的 scripts/ 目录执行脚本。
///
/// 包含路径遍历保护：脚本路径会被规范化并验证是否仍在技能根目录内。
pub struct RunSkillScriptTool {
    skills: Arc<Vec<AgentSkill>>,
    runner: Option<Arc<dyn AgentSkillScriptRunner>>,
}

impl RunSkillScriptTool {
    pub fn new(
        skills: Arc<Vec<AgentSkill>>,
        runner: Option<Arc<dyn AgentSkillScriptRunner>>,
    ) -> Self {
        Self { skills, runner }
    }
}

#[async_trait]
impl ITool for RunSkillScriptTool {
    fn name(&self) -> &str {
        "run_skill_script"
    }

    fn description(&self) -> &str {
        "Execute a script from a skill's scripts/ directory. Use this to run validation, analysis, or automation scripts bundled with a skill."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The name of the skill"
                },
                "script_path": {
                    "type": "string",
                    "description": "Relative path to the script file (e.g., 'scripts/validate.py')"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional command-line arguments to pass to the script"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds for the script (optional; defaults to 30). Increase for long-running scripts."
                }
            },
            "required": ["skill_name", "script_path"]
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
        let script_path = arguments["script_path"]
            .as_str()
            .ok_or_else(|| {
                rust_agent_core::AgentError::ToolError(
                    "Missing 'script_path' argument".into(),
                )
            })?;

        let skill = self.skills.iter().find(|s| s.metadata.name == skill_name).ok_or_else(|| {
            rust_agent_core::AgentError::ToolError(format!(
                "Skill '{}' not found",
                skill_name
            ))
        })?;

        let root = skill.root_dir.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::ConfigError(
                "Skill has no root_dir — script execution not supported for dynamic skills".into(),
            )
        })?;

        let full_path = root.join(script_path);

        // ── Path-traversal protection (FIX: previously missing) ──
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let canonical_path = full_path.canonicalize().unwrap_or_else(|_| full_path.clone());
        if !canonical_path.starts_with(&canonical_root) {
            return Err(rust_agent_core::AgentError::ToolError(
                "Path traversal denied — script path escapes skill directory".into(),
            ));
        }

        let script_args = if let Some(arr) = arguments.get("args").and_then(|v| v.as_array()) {
            Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
        } else {
            None
        };

        let runner = self.runner.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::ToolError(
                "No script runner configured".into(),
            )
        })?;

        let timeout_secs = arguments
            .get("timeout_secs")
            .and_then(|v| v.as_u64());

        let output = runner
            .run(skill_name, &full_path, script_args, timeout_secs)
            .await?;
        Ok(serde_json::json!({
            "ok": true,
            "data": {
                "skill_name": skill_name,
                "script_path": script_path,
                "output": output,
            }
        }).to_string())
    }
}

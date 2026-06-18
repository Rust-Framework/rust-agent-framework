use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextResult, IAgent, IContextProvider,
    ISession, ITool, Result,
};

use super::agent_skill::AgentSkill;

use crate::tools::{LoadSkillTool, ReadSkillResourceTool};

// ── AgentSkillsProvider ──

/// AgentSkillsProvider — IContextProvider 实现
///
/// 对标 MAF 的 AgentSkillsProvider (C#) / SkillsProvider (Python)。
///
/// 注入：
///   1. advertise 文本（技能名称 + 描述 + 路径指引）
///   2. load_skill / read_skill_resource 工具（按需）
pub struct AgentSkillsProvider {
    pub skills: Vec<AgentSkill>,
}

impl AgentSkillsProvider {
    pub fn new() -> Self {
        Self {
            skills: Vec::new(),
        }
    }

    pub fn with_skill(mut self, skill: AgentSkill) -> Self {
        self.skills.push(skill);
        self
    }

    pub fn with_skills(mut self, skills: impl IntoIterator<Item = AgentSkill>) -> Self {
        self.skills.extend(skills);
        self
    }

    /// 批量扫描目录，自动发现所有含 SKILL.md 的子目录。
    pub fn scan(root: impl AsRef<Path>) -> Result<Self> {
        let mut provider = Self::new();
        let root = root.as_ref();

        if !root.exists() || !root.is_dir() {
            return Ok(provider);
        }

        for entry in std::fs::read_dir(root).map_err(|e| {
            rust_agent_core::AgentError::ConfigError(format!(
                "Failed to read skill directory '{}': {}",
                root.display(),
                e
            ))
        })? {
            let entry = entry.map_err(|e| {
                rust_agent_core::AgentError::ConfigError(format!("Failed to read entry: {}", e))
            })?;
            let path = entry.path();
            if path.is_dir() && path.join("SKILL.md").exists() {
                let skill = AgentSkill::from_dir(&path)?;
                provider = provider.with_skill(skill);
            }
        }

        Ok(provider)
    }

    /// 从多个目录扫描。
    pub fn scan_dirs(roots: &[impl AsRef<Path>]) -> Result<Self> {
        let mut all_skills = Vec::new();
        for root in roots {
            let provider = Self::scan(root)?;
            all_skills.extend(provider.skills);
        }
        Ok(Self {
            skills: all_skills,
        })
    }

    // ── 内部工具创建 ──

    pub fn create_load_skill_tool(&self) -> Arc<dyn ITool> {
        Arc::new(LoadSkillTool {
            skills: Arc::new(self.skills.clone()),
        })
    }

    pub fn create_read_resource_tool(&self) -> Arc<dyn ITool> {
        Arc::new(ReadSkillResourceTool {
            skills: Arc::new(self.skills.clone()),
        })
    }

    // ── advertise 文本 ──

    pub fn build_advertise(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let mut text = String::from("## Available Skills\n");

        let has_resources = self.skills.iter().any(|s| s.has_resources());
        let has_scripts = self.skills.iter().any(|s| s.has_scripts());

        text.push_str("Use load_skill(name) to get full instructions for any skill listed below.\n\n");

        for skill in &self.skills {
            text.push_str(&format!(
                "- **{}**: {}\n",
                skill.metadata.name, skill.metadata.description
            ));

            if has_resources && skill.has_resources() {
                text.push_str("  Resources: use read_skill_resource(\"");
                text.push_str(&skill.metadata.name);
                text.push_str("\", \"<path>\") to read reference documents.\n");
            }

            if has_scripts && skill.has_scripts() {
                text.push_str("  Scripts: use run_command(\"python scripts/<script>\") with workspace scope set to the skill directory.\n");
            }
        }

        text
    }

    pub fn build_tools(&self) -> Vec<Arc<dyn ITool>> {
        let mut tools = vec![self.create_load_skill_tool()];

        let has_resources = self.skills.iter().any(|s| s.has_resources());
        if has_resources {
            tools.push(self.create_read_resource_tool());
        }

        tools
    }
}

impl Default for AgentSkillsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IContextProvider for AgentSkillsProvider {
    fn name(&self) -> &str {
        "AgentSkillsProvider"
    }

    async fn on_invoking(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _messages: &[ChatMessage],
        _options: &AgentRunOptions,
    ) -> Result<ContextResult> {
        Ok(ContextResult {
            instructions: Some(self.build_advertise()),
            tools: self.build_tools(),
            ..Default::default()
        })
    }

    async fn on_invoked(
        &self,
        _agent: &dyn IAgent,
        _session: &dyn ISession,
        _request_messages: &[ChatMessage],
        _response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        Ok(())
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_providers::agent_skill::SkillMetadata;

    #[test]
    fn test_provider_scan_empty() {
        let dir = tempfile::tempdir().unwrap();
        let provider = AgentSkillsProvider::scan(dir.path()).unwrap();
        assert!(provider.skills.is_empty());
    }

    #[test]
    fn test_provider_scan_with_skills() {
        let dir = tempfile::tempdir().unwrap();

        for name in &["skill-a", "skill-b"] {
            let skill_dir = dir.path().join(name);
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {}\ndescription: Desc for {}.\n---\n# Body", name, name),
            )
            .unwrap();
        }

        let provider = AgentSkillsProvider::scan(dir.path()).unwrap();
        assert_eq!(provider.skills.len(), 2);

        let names: Vec<&str> = provider
            .skills
            .iter()
            .map(|s| s.metadata.name.as_str())
            .collect();
        assert!(names.contains(&"skill-a"));
        assert!(names.contains(&"skill-b"));
    }

    #[test]
    fn test_advertise_text() {
        let skill_a = AgentSkill::dynamic(
            SkillMetadata {
                name: "alpha".into(),
                description: "Alpha skill.".into(),
                ..Default::default()
            },
            "# Alpha",
        )
        .with_resource("ref.md", "# Ref");

        let skill_b = AgentSkill::dynamic(
            SkillMetadata {
                name: "beta".into(),
                description: "Beta skill.".into(),
                ..Default::default()
            },
            "# Beta",
        );

        let provider = AgentSkillsProvider::new()
            .with_skill(skill_a)
            .with_skill(skill_b);

        let text = provider.build_advertise();
        assert!(text.contains("## Available Skills"));
        assert!(text.contains("alpha"));
        assert!(text.contains("Alpha skill"));
        assert!(text.contains("beta"));
        assert!(text.contains("Beta skill"));
        assert!(text.contains("load_skill"));
        assert!(text.contains("read_skill_resource"));
    }

    #[test]
    fn test_provider_build_tools() {
        let skill = AgentSkill::dynamic(
            SkillMetadata {
                name: "test".into(),
                description: "Test.".into(),
                ..Default::default()
            },
            "# Test",
        )
        .with_resource("ref.md", "# Ref");

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tools = provider.build_tools();

        assert!(tools.len() >= 2);
        assert!(tools.iter().any(|t| t.name() == "load_skill"));
        assert!(tools.iter().any(|t| t.name() == "read_skill_resource"));
        assert!(!tools.iter().any(|t| t.name() == "run_skill_script"));
    }

    #[test]
    fn test_provider_build_tools_no_resources() {
        let skill = AgentSkill::dynamic(
            SkillMetadata {
                name: "test".into(),
                description: "Test.".into(),
                ..Default::default()
            },
            "# Test",
        );

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tools = provider.build_tools();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "load_skill");
    }

    #[test]
    fn test_load_skill_tool_execute() {
        let skill = AgentSkill::dynamic(
            SkillMetadata {
                name: "my-skill".into(),
                description: "Test.".into(),
                ..Default::default()
            },
            "# Instructions\nDo the thing.",
        );

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tool = provider.create_load_skill_tool();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(tool.execute(serde_json::json!({"skill_name": "my-skill"})))
            .unwrap();
        assert!(result.ok);
        let data = result.data.as_ref().unwrap();
        assert_eq!(data["skill_name"], "my-skill");
        assert!(data["instructions"]
            .as_str()
            .unwrap()
            .contains("Do the thing"));
    }

    #[test]
    fn test_load_skill_tool_not_found() {
        let skill = AgentSkill::dynamic(
            SkillMetadata {
                name: "my-skill".into(),
                description: "Test.".into(),
                ..Default::default()
            },
            "# Test",
        );

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tool = provider.create_load_skill_tool();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(serde_json::json!({"skill_name": "nonexistent"})));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_read_resource_tool() {
        let skill = AgentSkill::dynamic(
            SkillMetadata {
                name: "s".into(),
                description: "T".into(),
                ..Default::default()
            },
            "# Test",
        )
        .with_resource("ref.md", "# Reference Content");

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tool = provider.create_read_resource_tool();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(tool.execute(serde_json::json!({
                "skill_name": "s",
                "resource_path": "ref.md"
            })))
            .unwrap();
        assert!(result.ok);
        let data = result.data.as_ref().unwrap();
        assert!(data["content"]
            .as_str()
            .unwrap()
            .contains("Reference Content"));
    }

}

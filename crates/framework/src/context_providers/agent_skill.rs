use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rust_agent_core::Result;

// ── Skill Metadata ──

/// 技能元信息（从 SKILL.md frontmatter 解析）
#[derive(Debug, Clone, Default)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: HashMap<String, String>,
}

// ── AgentSkill ──

/// AgentSkill — 一个技能对象，持有元数据 + 技能目录路径
///
/// 可对接多种来源：本地目录、数据库、远程 API 等。
#[derive(Clone)]
pub struct AgentSkill {
    pub metadata: SkillMetadata,
    /// 技能根目录（from_dir 时设置）
    pub(crate) root_dir: Option<PathBuf>,
    /// 自定义指令内容（dynamic 时设置，优先于文件读取）
    instructions: Option<String>,
    /// 自定义资源内容表（resource_path → content）
    resources: HashMap<String, Vec<u8>>,
}

impl std::fmt::Debug for AgentSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSkill")
            .field("metadata", &self.metadata)
            .field("root_dir", &self.root_dir)
            .field("has_instructions", &self.instructions.is_some())
            .field("resource_count", &self.resources.len())
            .finish()
    }
}

impl AgentSkill {
    /// 从目录加载。立即解析 SKILL.md frontmatter（元信息），
    /// 正文和资源文件延迟读取。
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let skill_md = path.join("SKILL.md");

        if !skill_md.exists() {
            return Err(rust_agent_core::AgentError::ConfigError(format!(
                "SKILL.md not found in {}",
                path.display()
            )));
        }

        let content = std::fs::read_to_string(&skill_md).map_err(|e| {
            rust_agent_core::AgentError::ConfigError(format!(
                "Failed to read SKILL.md: {}",
                e
            ))
        })?;

        let metadata = Self::parse_frontmatter(&content)?;

        Ok(Self {
            metadata,
            root_dir: Some(path.to_path_buf()),
            instructions: None,
            resources: HashMap::new(),
        })
    }

    /// 动态创建（用于数据库 / 远程等定制场景）。
    pub fn dynamic(metadata: SkillMetadata, instructions: impl Into<String>) -> Self {
        Self {
            metadata,
            root_dir: None,
            instructions: Some(instructions.into()),
            resources: HashMap::new(),
        }
    }

    /// 添加内联资源（用于动态技能）。
    pub fn with_resource(mut self, path: &str, content: impl Into<Vec<u8>>) -> Self {
        self.resources.insert(path.to_string(), content.into());
        self
    }

    /// 加载技能的完整指令内容（SKILL.md 正文 或 dynamic 指令）。
    pub(crate) fn load_instructions(&self) -> Result<String> {
        // dynamic 创建的技能：直接返回存储的指令
        if let Some(ref instructions) = self.instructions {
            return Ok(instructions.clone());
        }

        // from_dir 创建：读取 SKILL.md 并剥离 frontmatter
        let root = self.root_dir.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::ConfigError(
                "Skill has no root_dir and no inline instructions".into(),
            )
        })?;
        let skill_md = root.join("SKILL.md");
        let content = std::fs::read_to_string(&skill_md).map_err(|e| {
            rust_agent_core::AgentError::ConfigError(format!("Failed to read SKILL.md: {}", e))
        })?;
        Ok(Self::strip_frontmatter(&content))
    }

    /// 读取技能资源文件内容。
    pub(crate) fn read_resource(&self, resource_path: &str) -> Result<String> {
        // 优先检查内联资源（dynamic 技能）
        if let Some(data) = self.resources.get(resource_path) {
            return String::from_utf8(data.clone())
                .map_err(|e| rust_agent_core::AgentError::ConfigError(format!(
                    "Resource is not valid UTF-8: {}", e
                )));
        }

        // 从目录读取
        let root = self.root_dir.as_ref().ok_or_else(|| {
            rust_agent_core::AgentError::ConfigError(
                "Skill has no root_dir".into(),
            )
        })?;

        // 安全检查 + 路径解析：使用统一的 path_guard 避免相对/绝对路径比较 bug
        let resolved = crate::tools::path_guard::resolve_safe(root, resource_path)?;

        std::fs::read_to_string(&resolved).map_err(|e| {
            rust_agent_core::AgentError::ToolError(format!(
                "Failed to read resource '{}': {}",
                resource_path, e
            ))
        })
    }

    /// 检查技能是否有资源（references/assets 或内联 resources）。
    pub fn has_resources(&self) -> bool {
        if !self.resources.is_empty() {
            return true;
        }
        if let Some(ref root) = self.root_dir {
            for dir in &["references", "assets"] {
                let d = root.join(dir);
                if d.exists() && d.is_dir() {
                    return true;
                }
            }
        }
        false
    }

    /// 检查技能是否有可执行脚本。
    pub fn has_scripts(&self) -> bool {
        if let Some(ref root) = self.root_dir {
            let scripts_dir = root.join("scripts");
            if scripts_dir.exists() && scripts_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&scripts_dir) {
                    return entries.count() > 0;
                }
            }
        }
        false
    }

    /// 解析 YAML frontmatter（仅支持简单 key: value 格式）。
    fn parse_frontmatter(content: &str) -> Result<SkillMetadata> {
        let content = content.trim_start();
        if !content.starts_with("---") {
            return Ok(SkillMetadata::default());
        }

        let rest = &content[3..];
        let end = rest.find("---").unwrap_or(rest.len());
        let fm = &rest[..end];

        let mut name = String::new();
        let mut description = String::new();
        let mut license: Option<String> = None;
        let mut compatibility: Option<String> = None;
        let mut metadata = HashMap::new();
        let mut in_metadata = false;

        for line in fm.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if in_metadata {
                if let Some((k, v)) = Self::parse_kv(line) {
                    metadata.insert(k, v);
                }
                continue;
            }

            match line {
                s if s.starts_with("metadata:") => {
                    in_metadata = true;
                }
                s if s.starts_with("name:") => {
                    name = s[5..].trim().trim_matches('"').to_string();
                }
                s if s.starts_with("description:") => {
                    description = s[12..].trim().trim_matches('"').to_string();
                    // 处理多行描述（> 或 | 后跟缩进）
                    if description == ">" || description == "|" {
                        description = String::new();
                        // 简化处理：取下一行
                    }
                }
                s if s.starts_with("license:") => {
                    license = Some(s[8..].trim().trim_matches('"').to_string());
                }
                s if s.starts_with("compatibility:") => {
                    compatibility = Some(s[15..].trim().trim_matches('"').to_string());
                }
                _ => {}
            }
        }

        Ok(SkillMetadata {
            name,
            description,
            license,
            compatibility,
            metadata,
        })
    }

    /// 从内容中移除 YAML frontmatter，返回纯 Markdown 正文。
    pub(crate) fn strip_frontmatter(content: &str) -> String {
        let content = content.trim_start();
        if !content.starts_with("---") {
            return content.to_string();
        }
        let rest = &content[3..];
        if let Some(end) = rest.find("---") {
            rest[end + 3..].trim().to_string()
        } else {
            content.to_string()
        }
    }

    fn parse_kv(line: &str) -> Option<(String, String)> {
        let pos = line.find(':')?;
        let key = line[..pos].trim().to_string();
        let value = line[pos + 1..].trim().trim_matches('"').to_string();
        Some((key, value))
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_basic() {
        let content = r#"---
name: code-review
description: Review code for quality and bugs.
license: Apache-2.0
compatibility: Any
metadata:
  author: team-a
  version: "1.0"
---

# Instructions
1. Read the code.
2. Check for issues.
"#;

        let meta = AgentSkill::parse_frontmatter(content).unwrap();
        assert_eq!(meta.name, "code-review");
        assert_eq!(meta.description, "Review code for quality and bugs.");
        assert_eq!(meta.license.as_deref(), Some("Apache-2.0"));
        assert_eq!(meta.compatibility.as_deref(), Some("Any"));
        assert_eq!(meta.metadata.get("author").map(|s| s.as_str()), Some("team-a"));
        assert_eq!(meta.metadata.get("version").map(|s| s.as_str()), Some("1.0"));
    }

    #[test]
    fn test_strip_frontmatter() {
        let content = r#"---
name: test
---
# Body
Content here."#;

        let body = AgentSkill::strip_frontmatter(content);
        assert_eq!(body, "# Body\nContent here.");
    }

    #[test]
    fn test_parse_frontmatter_minimal() {
        let content = r#"---
name: minimal
description: Just the minimum.
---
Body"#;

        let meta = AgentSkill::parse_frontmatter(content).unwrap();
        assert_eq!(meta.name, "minimal");
        assert_eq!(meta.description, "Just the minimum.");
    }

    #[test]
    fn test_parse_frontmatter_no_frontmatter() {
        let content = "# Just markdown, no frontmatter";
        let meta = AgentSkill::parse_frontmatter(content).unwrap();
        assert_eq!(meta.name, "");
        assert_eq!(meta.description, "");
    }

    #[test]
    fn test_skill_dynamic() {
        let skill = AgentSkill::dynamic(
            SkillMetadata {
                name: "test-skill".into(),
                description: "A test skill".into(),
                ..Default::default()
            },
            "# Instructions\nTest instructions.",
        )
        .with_resource("ref.md", "# Reference\nRef content.");

        assert_eq!(skill.metadata.name, "test-skill");
        assert!(skill.has_resources());

        let instructions = skill.load_instructions().unwrap();
        assert!(instructions.contains("Test instructions"));

        let resource = skill.read_resource("ref.md").unwrap();
        assert!(resource.contains("Ref content"));
    }

    #[test]
    fn test_skill_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();

        let skill_md = r#"---
name: test-skill
description: A test.
---
# Test
Hello world."#;
        std::fs::write(skill_dir.join("SKILL.md"), skill_md).unwrap();

        let skill = AgentSkill::from_dir(&skill_dir).unwrap();
        assert_eq!(skill.metadata.name, "test-skill");
        assert_eq!(skill.metadata.description, "A test.");

        let instructions = skill.load_instructions().unwrap();
        assert!(instructions.contains("Hello world"));
        assert!(!instructions.contains("---"));
    }

    #[test]
    fn test_skill_has_resources() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::create_dir_all(skill_dir.join("references")).unwrap();

        std::fs::write(skill_dir.join("SKILL.md"), "---\nname: test\ndescription: Test.\n---\nBody").unwrap();
        std::fs::write(skill_dir.join("references").join("guide.md"), "# Guide").unwrap();

        let skill = AgentSkill::from_dir(&skill_dir).unwrap();
        assert!(skill.has_resources());
        assert!(!skill.has_scripts());
    }

    #[test]
    fn test_skill_has_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();

        std::fs::write(skill_dir.join("SKILL.md"), "---\nname: test\ndescription: Test.\n---\nBody").unwrap();
        std::fs::write(skill_dir.join("scripts").join("run.sh"), "echo hello").unwrap();

        let skill = AgentSkill::from_dir(&skill_dir).unwrap();
        assert!(skill.has_scripts());
    }
}

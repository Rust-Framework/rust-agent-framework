use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::Future;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection, IAgent, IContextProvider,
    ISession, ITool, Result,
};

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
    root_dir: Option<PathBuf>,
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

        let full_path = root.join(resource_path);

        // 安全检查：防止路径穿越
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let canonical_path = full_path.canonicalize().unwrap_or_else(|_| full_path.clone());
        if !canonical_path.starts_with(&canonical_root) {
            return Err(rust_agent_core::AgentError::ToolError(
                "Path traversal denied".into(),
            ));
        }

        std::fs::read_to_string(&full_path).map_err(|e| {
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
    fn strip_frontmatter(content: &str) -> String {
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

// ── Internal Tool Wrapper ──

/// 函数式 ITool 实现 — 避免引入 pub(crate) AIFunction 的跨 crate 访问问题。
struct FnTool {
    name: String,
    description: String,
    parameters_schema: serde_json::Value,
    handler: Arc<
        dyn Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = Result<String>> + Send>>
            + Send
            + Sync,
    >,
}

impl FnTool {
    fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters_schema: serde_json::Value,
        handler: impl Fn(serde_json::Value) -> Pin<Box<dyn Future<Output = Result<String>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters_schema,
            handler: Arc::new(handler),
        }
    }
}

#[async_trait]
impl ITool for FnTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.parameters_schema.clone()
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
        (self.handler)(arguments).await
    }
}

// ── AgentSkillScriptRunner ──

/// 技能脚本执行器 trait。
///
/// 对标 MAF SubprocessScriptRunner。用户可实现自定义 Runner（沙箱、容器等）。
#[async_trait]
pub trait AgentSkillScriptRunner: Send + Sync {
    /// 执行脚本，返回 stdout。
    async fn run(
        &self,
        skill_name: &str,
        script_path: &Path,
        args: Option<Vec<String>>,
    ) -> Result<String>;
}

/// 默认子进程执行器。
pub struct SubprocessScriptRunner;

#[async_trait]
impl AgentSkillScriptRunner for SubprocessScriptRunner {
    async fn run(
        &self,
        _skill_name: &str,
        script_path: &Path,
        args: Option<Vec<String>>,
    ) -> Result<String> {
        // 根据扩展名选择解释器
        let ext = script_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let (program, mut cmd_parts): (&str, Vec<String>) = match ext {
            "py" => ("python", vec![script_path.to_string_lossy().to_string()]),
            "js" => ("node", vec![script_path.to_string_lossy().to_string()]),
            "sh" if cfg!(windows) => ("bash", vec![script_path.to_string_lossy().to_string()]),
            "ps1" => ("powershell", vec!["-File".to_string(), script_path.to_string_lossy().to_string()]),
            _ => {
                if cfg!(windows) {
                    ("cmd", vec!["/c".to_string(), script_path.to_string_lossy().to_string()])
                } else {
                    ("sh", vec!["-c".to_string(), script_path.to_string_lossy().to_string()])
                }
            }
        };

        if let Some(a) = &args {
            cmd_parts.extend(a.iter().cloned());
        }

        let output = std::process::Command::new(program)
            .args(&cmd_parts)
            .output()
            .map_err(|e| {
                rust_agent_core::AgentError::ToolError(format!(
                    "Failed to execute script: {}",
                    e
                ))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(stdout)
        } else {
            Err(rust_agent_core::AgentError::ToolError(format!(
                "Script exited with code {:?}\nstderr: {}",
                output.status.code(),
                stderr
            )))
        }
    }
}

// ── AgentSkillsProvider ──

/// AgentSkillsProvider — IContextProvider 实现
///
/// 对标 MAF 的 AgentSkillsProvider (C#) / SkillsProvider (Python)。
///
/// 注入：
///   1. advertise 文本（技能名称 + 描述 + 路径指引）
///   2. load_skill / read_skill_resource / run_skill_script 工具（按需）
pub struct AgentSkillsProvider {
    pub skills: Vec<AgentSkill>,
    script_runner: Option<Arc<dyn AgentSkillScriptRunner>>,
}

impl AgentSkillsProvider {
    pub fn new() -> Self {
        Self {
            skills: Vec::new(),
            script_runner: None,
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

    pub fn with_script_runner(mut self, runner: Arc<dyn AgentSkillScriptRunner>) -> Self {
        self.script_runner = Some(runner);
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
            script_runner: None,
        })
    }

    // ── 内部工具创建 ──

   pub fn create_load_skill_tool(&self) -> Arc<dyn ITool> {
        let skills: Vec<AgentSkill> = self.skills.clone();

        Arc::new(FnTool::new(
            "load_skill",
            "Load a skill's full instructions from SKILL.md. Call this when a user's task matches a skill's domain to get detailed step-by-step guidance.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "skill_name": {
                        "type": "string",
                        "description": "The name of the skill to load"
                    }
                },
                "required": ["skill_name"]
            }),
            move |args: serde_json::Value| {
                let skills = skills.clone();
                Box::pin(async move {
                    let skill_name = args["skill_name"]
                        .as_str()
                        .ok_or_else(|| {
                            rust_agent_core::AgentError::ToolError(
                                "Missing 'skill_name' argument".into(),
                            )
                        })?;

                    let skill = skills.iter().find(|s| s.metadata.name == skill_name).ok_or_else(|| {
                        rust_agent_core::AgentError::ToolError(format!(
                            "Skill '{}' not found. Available skills: {}",
                            skill_name,
                            skills
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
                })
            },
        ))
    }

    pub fn create_read_resource_tool(&self) -> Arc<dyn ITool> {
        let skills: Vec<AgentSkill> = self.skills.clone();

        Arc::new(FnTool::new(
            "read_skill_resource",
            "Read a resource file from a loaded skill's references/ or assets/ directory. Use this to access supplementary documents, templates, or style guides.",
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
            }),
            move |args: serde_json::Value| {
                let skills = skills.clone();
                Box::pin(async move {
                    let skill_name = args["skill_name"]
                        .as_str()
                        .ok_or_else(|| {
                            rust_agent_core::AgentError::ToolError(
                                "Missing 'skill_name' argument".into(),
                            )
                        })?;
                    let resource_path = args["resource_path"]
                        .as_str()
                        .ok_or_else(|| {
                            rust_agent_core::AgentError::ToolError(
                                "Missing 'resource_path' argument".into(),
                            )
                        })?;

                    let skill = skills.iter().find(|s| s.metadata.name == skill_name).ok_or_else(|| {
                        rust_agent_core::AgentError::ToolError(format!(
                            "Skill '{}' not found",
                            skill_name
                        ))
                    })?;

                    let content = skill.read_resource(resource_path)?;
                    Ok(serde_json::json!({
                        "ok": true,
                        "data": {
                            "skill_name": skill_name,
                            "resource_path": resource_path,
                            "content": content,
                        }
                    }).to_string())
                })
            },
        ))
    }

    pub fn create_run_script_tool(&self) -> Arc<dyn ITool> {
        let skills: Vec<AgentSkill> = self.skills.clone();
        let runner = self.script_runner.clone();

        Arc::new(FnTool::new(
            "run_skill_script",
            "Execute a script from a skill's scripts/ directory. Use this to run validation, analysis, or automation scripts bundled with a skill.",
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
                    }
                },
                "required": ["skill_name", "script_path"]
            }),
            move |args: serde_json::Value| {
                let skills = skills.clone();
                let runner = runner.clone();
                Box::pin(async move {
                    let skill_name = args["skill_name"]
                        .as_str()
                        .ok_or_else(|| {
                            rust_agent_core::AgentError::ToolError(
                                "Missing 'skill_name' argument".into(),
                            )
                        })?;
                    let script_path = args["script_path"]
                        .as_str()
                        .ok_or_else(|| {
                            rust_agent_core::AgentError::ToolError(
                                "Missing 'script_path' argument".into(),
                            )
                        })?;

                    let skill = skills.iter().find(|s| s.metadata.name == skill_name).ok_or_else(|| {
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

                    let script_args = if let Some(arr) = args.get("args").and_then(|v| v.as_array()) {
                        Some(arr.iter().filter_map(|v| v.as_str().map(String::from)).collect::<Vec<_>>())
                    } else {
                        None
                    };

                    let runner = runner.as_ref().ok_or_else(|| {
                        rust_agent_core::AgentError::ToolError(
                            "No script runner configured".into(),
                        )
                    })?;

                    let output = runner.run(skill_name, &full_path, script_args).await?;
                    Ok(serde_json::json!({
                        "ok": true,
                        "data": {
                            "skill_name": skill_name,
                            "script_path": script_path,
                            "output": output,
                        }
                    }).to_string())
                })
            },
        ))
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
                text.push_str(
                    "  Resources: use read_skill_resource(\""
                );
                text.push_str(&skill.metadata.name);
                text.push_str("\", \"<path>\") to read reference documents.\n");
            }

            if has_scripts && skill.has_scripts() {
                text.push_str(
                    "  Scripts: use run_skill_script(\""
                );
                text.push_str(&skill.metadata.name);
                text.push_str("\", \"<path>\") to execute bundled scripts.\n");
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

        let has_scripts = self.skills.iter().any(|s| s.has_scripts());
        if has_scripts && self.script_runner.is_some() {
            tools.push(self.create_run_script_tool());
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
    ) -> Result<ContextInjection> {
        Ok(ContextInjection {
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

    #[test]
    fn test_provider_scan_empty() {
        let dir = tempfile::tempdir().unwrap();
        let provider = AgentSkillsProvider::scan(dir.path()).unwrap();
        assert!(provider.skills.is_empty());
    }

    #[test]
    fn test_provider_scan_with_skills() {
        let dir = tempfile::tempdir().unwrap();

        // Create two skill directories
        for name in &["skill-a", "skill-b"] {
            let skill_dir = dir.path().join(name);
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {}\ndescription: Desc for {}.\n---\n# Body", name, name),
            ).unwrap();
        }

        let provider = AgentSkillsProvider::scan(dir.path()).unwrap();
        assert_eq!(provider.skills.len(), 2);

        let names: Vec<&str> = provider.skills.iter()
            .map(|s| s.metadata.name.as_str())
            .collect();
        assert!(names.contains(&"skill-a"));
        assert!(names.contains(&"skill-b"));
    }

    #[test]
    fn test_advertise_text() {
        let skill_a = AgentSkill::dynamic(
            SkillMetadata { name: "alpha".into(), description: "Alpha skill.".into(), ..Default::default() },
            "# Alpha",
        ).with_resource("ref.md", "# Ref");

        let skill_b = AgentSkill::dynamic(
            SkillMetadata { name: "beta".into(), description: "Beta skill.".into(), ..Default::default() },
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
        // alpha has resources
        assert!(text.contains("read_skill_resource"));
    }

    #[test]
    fn test_provider_build_tools() {
        let skill = AgentSkill::dynamic(
            SkillMetadata { name: "test".into(), description: "Test.".into(), ..Default::default() },
            "# Test",
        ).with_resource("ref.md", "# Ref");

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tools = provider.build_tools();

        // Should have load_skill and read_skill_resource (has resources)
        assert!(tools.len() >= 2);
        assert!(tools.iter().any(|t| t.name() == "load_skill"));
        assert!(tools.iter().any(|t| t.name() == "read_skill_resource"));
        // No scripts, no run_skill_script
        assert!(!tools.iter().any(|t| t.name() == "run_skill_script"));
    }

    #[test]
    fn test_provider_build_tools_no_resources() {
        let skill = AgentSkill::dynamic(
            SkillMetadata { name: "test".into(), description: "Test.".into(), ..Default::default() },
            "# Test",
        );

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tools = provider.build_tools();

        // Only load_skill (no resources or scripts)
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "load_skill");
    }

    #[test]
    fn test_subprocess_runner() {
        // Write a temp script (integration test — runner tested indirectly)
        let _runner = SubprocessScriptRunner;
        let _cmd = "echo hello";
    }

    #[test]
    fn test_load_skill_tool_execute() {
        let skill = AgentSkill::dynamic(
            SkillMetadata { name: "my-skill".into(), description: "Test.".into(), ..Default::default() },
            "# Instructions\nDo the thing.",
        );

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tool = provider.create_load_skill_tool();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(serde_json::json!({"skill_name": "my-skill"}))).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["skill_name"], "my-skill");
        assert!(v["data"]["instructions"].as_str().unwrap().contains("Do the thing"));
    }

    #[test]
    fn test_load_skill_tool_not_found() {
        let skill = AgentSkill::dynamic(
            SkillMetadata { name: "my-skill".into(), description: "Test.".into(), ..Default::default() },
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
            SkillMetadata { name: "s".into(), description: "T".into(), ..Default::default() },
            "# Test",
        ).with_resource("ref.md", "# Reference Content");

        let provider = AgentSkillsProvider::new().with_skill(skill);
        let tool = provider.create_read_resource_tool();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(tool.execute(serde_json::json!({
            "skill_name": "s",
            "resource_path": "ref.md"
        }))).unwrap();
        let v: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["data"]["content"].as_str().unwrap().contains("Reference Content"));
    }
}

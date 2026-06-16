use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection, IAgent, IContextProvider,
    IChatClient, ISession, ITool, Result,
};

use crate::context_providers::agent_skill::AgentSkill;
use crate::context_providers::skills_provider::AgentSkillsProvider;
use super::memory_agent::run_memory_agent;

/// SkillMemoryContextProvider — 技能记忆上下文提供器
///
/// 管理一个由 `SKILL.md` 定义的记忆技能，工具复用 `AgentSkillsProvider` 的
/// `load_skill` 和 `read_skill_resource`。
/// 在 `on_invoked` 中按配置的间隔调用 MemoryAgent 进行记忆沉淀。
pub struct SkillMemoryContextProvider {
    /// 是否启用记忆功能（默认 true）
    enabled: bool,
    /// memory 数据存储目录（包含 SKILL.md、references/、assets/）
    memory_dir: PathBuf,
    /// 持有 AgentSkillsProvider 复用其工具注册逻辑
    skills_provider: Option<AgentSkillsProvider>,
    /// MemoryAgent 使用的 LLM chat_client（可选，无则不启用）
    memory_agent_client: Option<Arc<dyn IChatClient>>,
    /// MemoryAgent 执行间隔（每 N 轮执行一次，默认 3，0 禁用）
    consolidation_interval: usize,
}

impl SkillMemoryContextProvider {
    pub fn new(memory_dir: impl AsRef<Path>) -> Self {
        let memory_dir_buf = memory_dir.as_ref().to_path_buf();
        let skills_provider = AgentSkill::from_dir(&memory_dir_buf)
            .ok()
            .map(|skill| AgentSkillsProvider::new().with_skill(skill));

        Self {
            enabled: true,
            memory_dir: memory_dir_buf,
            skills_provider,
            memory_agent_client: None,
            consolidation_interval: 3,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_memory_agent(mut self, client: Arc<dyn IChatClient>) -> Self {
        self.memory_agent_client = Some(client);
        self
    }

    pub fn with_consolidation_interval(mut self, interval: usize) -> Self {
        self.consolidation_interval = interval;
        self
    }

    /// 从 LLM 视角优化：以规则体形式让 LLM 更容易注意和内化。
    fn build_advertise(&self) -> String {
        if !self.enabled {
            return String::new();
        }
        format!(
            "**MEMORY RULE:** You have persistent memory organized as files. \
             Whenever you need to recall prior context — user preferences, past \
             decisions, ongoing tasks — ALWAYS call `load_skill(\"skill-memory\")` \
             first to load the navigation index. Then use \
             `read_skill_resource(\"skill-memory\", \"<file>\")` to read the \
             specific file you need. Do not guess or assume; the files are your \
             authoritative memory.\n"
        )
    }

    /// 委托 AgentSkillsProvider 构建工具。
    /// references/ 和 assets/ 目录存在，has_resources() 返回 true，
    /// 因此 load_skill + read_skill_resource 都会被注册。
    fn build_tools(&self) -> Vec<Arc<dyn ITool>> {
        match &self.skills_provider {
            Some(p) => p.build_tools(),
            None => Vec::new(),
        }
    }
}

// ── IContextProvider 实现 ──

#[async_trait]
impl IContextProvider for SkillMemoryContextProvider {
    fn name(&self) -> &str {
        "SkillMemoryContextProvider"
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
        session: &dyn ISession,
        request_messages: &[ChatMessage],
        response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        // 检查是否启用 MemoryAgent
        let client = match &self.memory_agent_client {
            Some(c) => c.clone(),
            None => return Ok(()),
        };
        if self.consolidation_interval == 0 {
            return Ok(());
        }

        // 跟踪 turn count
        let provider_name = self.name().to_string();
        let key = format!("{}_turn_count", provider_name);

        let current_count = session
            .get_provider_state(&key)
            .ok()
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let new_count = current_count + 1;

        // 仅当达到间隔时运行 MemoryAgent
        if new_count >= self.consolidation_interval {
            // 重置计数
            let _ = session.set_provider_state(&key, serde_json::Value::Number(0.into()));

            let memory_dir = self.memory_dir.clone();
            let messages = request_messages.to_vec();
            let resp_text = response.map(|r| r.text.clone());

            tokio::spawn(async move {
                run_memory_agent(memory_dir, client, messages, resp_text).await;
            });
        } else {
            let _ = session.set_provider_state(
                &key,
                serde_json::Value::Number(new_count.into()),
            );
        }

        Ok(())
    }
}

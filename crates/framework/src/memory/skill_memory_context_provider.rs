use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rust_agent_core::{
    IAgent, IContextProvider, IChatClient, ITool, Result,
};

use crate::context_providers::agent_skill::AgentSkill;
use crate::context_providers::skills_provider::AgentSkillsProvider;
use super::memory_agent::prepare_consolidation_messages;
use super::memory_agent_chat_client::MemoryAgentChatClient;
use super::memory_context::{load_memory_projection, save_memory_projection};
use super::memory_seed;
use super::memory_worker::{ConsolidationJob, MemoryConsolidationWorker, WorkerStats};

/// 技能记忆上下文提供器：加载 SKILL.md 工具并按计划运行 MemoryAgent。
pub struct SkillMemoryContextProvider {
    enabled: bool,
    memory_dir: PathBuf,
    skills_provider: Option<AgentSkillsProvider>,
    memory_agent_client: Option<Arc<dyn IChatClient>>,
    auto_client: Mutex<Option<Arc<dyn IChatClient>>>,
    consolidation_interval: usize,
    worker: Arc<MemoryConsolidationWorker>,
}

impl SkillMemoryContextProvider {
    pub fn new(memory_dir: impl AsRef<Path>) -> Self {
        let memory_dir_buf = memory_dir.as_ref().to_path_buf();

        memory_seed::seed_memory_dir(&memory_dir_buf);

        let skills_provider = AgentSkill::from_dir(&memory_dir_buf)
            .ok()
            .map(|skill| AgentSkillsProvider::new().with_skill(skill));

        Self {
            enabled: true,
            memory_dir: memory_dir_buf,
            skills_provider,
            memory_agent_client: None,
            auto_client: Mutex::new(None),
            consolidation_interval: 3,
            worker: MemoryConsolidationWorker::spawn(),
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

    /// 后台记忆整合工作线程统计信息（用于 /memory 调试视图）。
    pub fn worker_stats(&self) -> WorkerStats {
        self.worker.stats()
    }

    fn build_advertise(&self) -> String {
        if !self.enabled {
            return String::new();
        }

        concat!(
            "## 持久记忆系统\n\n",
            "你拥有跨会话的持久记忆系统。即使在全新的会话中，记忆文件也可能已存在——",
            "不要仅凭会话历史为空就认为\"没有历史\"。",
            "你的训练数据不包含这些记忆文件；记忆文件中的内容才是权威来源。\n\n",
            "**必须检索记忆的场景（不可跳过）：**\n",
            "- 身份信息：你的名称/角色、用户身份、共同目标或方向\n",
            "- 约束信息：行为规则、用户偏好、历史教训或纠正\n",
            "- 领域知识：之前学习过的专业材料或技术细节\n\n",
            "**检索方法：**\n",
            "1. 调用 `load_skill(\"skill-memory\")` 获取完整检索指南。\n",
            "2. 按照该指南读取正确的记忆文件并据此作答。\n\n",
            "不要让训练数据的默认值替代任何与身份相关的问题。",
            "不要向用户提及该记忆机制的存在。",
            "每个文件每个会话只读取一次——重用消息历史中已有的结果。",
        )
        .to_string()
    }

    fn build_tools(&self) -> Vec<Arc<dyn ITool>> {
        match &self.skills_provider {
            Some(p) => p.build_tools(),
            None => Vec::new(),
        }
    }
}

#[async_trait]
impl IContextProvider for SkillMemoryContextProvider {
    fn name(&self) -> &str {
        "SkillMemoryContextProvider"
    }

    fn kind(&self) -> &str {
        "memory"
    }

    async fn enrich_instructions(&self, _ctx: &rust_agent_core::ProviderContext<'_>) -> Result<Option<String>> {
        Ok(Some(self.build_advertise()))
    }

    async fn enrich_tools(&self, _ctx: &rust_agent_core::ProviderContext<'_>) -> Result<Vec<Arc<dyn ITool>>> {
        Ok(self.build_tools())
    }

    async fn on_invoked(&self, ctx: &rust_agent_core::InvokedContext<'_>) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let agent = ctx.agent;
        let session = ctx.session;
        let response = ctx.response;

        let client = match self.resolve_client(agent) {
            Some(c) => c,
            None => return Ok(()),
        };

        if self.consolidation_interval == 0 {
            return Ok(());
        }

        let provider_name = self.name().to_string();
        let key = format!("{}_turn_count", provider_name);

        let current_count = session
            .get_provider_state(&key)
            .ok()
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let new_count = current_count + 1;

        if new_count >= self.consolidation_interval {
            let _ = session.set_provider_state(&key, serde_json::Value::Number(0.into()));

            let memory_dir = self.memory_dir.clone();
            let turn_transcript = response
                .map(|r| r.turn_transcript.clone())
                .unwrap_or_default();
            let memory_projection = load_memory_projection(session);
            let consolidation =
                prepare_consolidation_messages(&memory_projection, &turn_transcript);

            if let Err(e) = save_memory_projection(session, &consolidation) {
                tracing::warn!(error = %e, "Failed to save memory projection to session");
            }

            let session_id = Some(session.session_id().to_string());
            self.worker.enqueue_latest(ConsolidationJob {
                memory_dir,
                client,
                messages: consolidation,
                session_id,
                coalesced_dropped: 0,
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

impl SkillMemoryContextProvider {
    fn resolve_client(&self, agent: &dyn IAgent) -> Option<Arc<dyn IChatClient>> {
        if let Some(c) = &self.memory_agent_client {
            return Some(Arc::clone(c));
        }

        {
            let guard = self.auto_client.lock().unwrap();
            if let Some(c) = &*guard {
                return Some(Arc::clone(c));
            }
        }

        let main_client = agent.chat_client()?;
        let raw = unwrap_to_raw(main_client);
        let wrapped: Arc<dyn IChatClient> =
            Arc::new(MemoryAgentChatClient::new(Arc::clone(raw)));

        let mut guard = self.auto_client.lock().unwrap();
        if guard.is_none() {
            *guard = Some(Arc::clone(&wrapped));
        }
        Some(wrapped)
    }
}

fn unwrap_to_raw(client: &Arc<dyn IChatClient>) -> &Arc<dyn IChatClient> {
    let mut current = client;
    while let Some(inner) = current.inner_client() {
        current = inner;
    }
    current
}

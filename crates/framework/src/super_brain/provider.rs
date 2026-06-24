use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rust_agent_core::{
    IAgent, IContextProvider, IChatClient, ITool, Result,
};

use crate::context::skill::AgentSkill;
use crate::tools::{LoadSkillTool, ReadSkillResourceTool};

use super::okf::{validate_super_brain, SuperBrainValidationReport};
use super::curator::{
    load_projection, prepare_consolidation_messages, save_projection, wrap_super_brain_curator_client,
    ConsolidationJob, ConsolidationWorker, WorkerStats,
};
use rust_agent_client::{
    clone_leaf_with_timeout, curator_timeout_secs, unwrap_chat_client_leaf,
};
use super::seed;

/// Super Brain 上下文提供器：读路径注入 SKILL 导航，写路径触发后台记忆整理子代理。
pub struct SuperBrainContextProvider {
    enabled: bool,
    super_brain_dir: PathBuf,
    skill: Option<AgentSkill>,
    curator_client: Option<Arc<dyn IChatClient>>,
    auto_client: Mutex<Option<Arc<dyn IChatClient>>>,
    consolidation_interval: usize,
    worker: Arc<ConsolidationWorker>,
}

impl SuperBrainContextProvider {
    pub fn new(super_brain_dir: impl AsRef<Path>) -> Self {
        let dir = super_brain_dir.as_ref().to_path_buf();

        seed::seed_super_brain_dir(&dir);

        let skill = AgentSkill::from_dir(&dir).ok();

        Self {
            enabled: true,
            super_brain_dir: dir,
            skill,
            curator_client: None,
            auto_client: Mutex::new(None),
            consolidation_interval: 3,
            worker: ConsolidationWorker::spawn(),
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_curator_client(mut self, client: Arc<dyn IChatClient>) -> Self {
        self.curator_client = Some(client);
        self
    }

    pub fn with_consolidation_interval(mut self, interval: usize) -> Self {
        self.consolidation_interval = interval;
        self
    }

    pub fn super_brain_dir(&self) -> &Path {
        &self.super_brain_dir
    }

    pub fn validate(&self) -> SuperBrainValidationReport {
        validate_super_brain(&self.super_brain_dir)
    }

    pub fn worker_stats(&self) -> WorkerStats {
        self.worker.stats()
    }

    fn build_advertise(&self) -> String {
        if !self.enabled {
            return String::new();
        }

        concat!(
            "## Super Brain 持久记忆\n\n",
            "你拥有跨会话的 Super Brain 持久记忆。即使在全新会话中，记忆文件也可能已存在——",
            "不要仅凭会话历史为空就认为\"没有历史\"。",
            "训练数据不包含这些文件；super-brain 中的内容才是权威来源。\n\n",
            "**必须检索记忆的场景（不可跳过）：**\n",
            "- 身份信息：你的名称/角色、用户身份、共同目标或方向\n",
            "- 约束信息：行为规则、用户偏好、历史教训或纠正\n",
            "- 领域知识：之前学习过的专业材料或技术细节\n\n",
            "**检索方法：**\n",
            "1. 调用 `load_skill(\"super-brain\")` 获取完整检索指南。\n",
            "2. 按照该指南读取正确的记忆文件并据此作答。\n\n",
            "不要让训练数据的默认值替代任何与身份相关的问题。",
            "不要向用户提及该记忆机制的存在。",
            "每个文件每个会话只读取一次——重用消息历史中已有的结果。",
        )
        .to_string()
    }

    fn build_tools(&self) -> Vec<Arc<dyn ITool>> {
        let Some(skill) = &self.skill else {
            return Vec::new();
        };
        let skills = Arc::new(vec![skill.clone()]);
        vec![
            Arc::new(LoadSkillTool {
                skills: Arc::clone(&skills),
            }),
            Arc::new(ReadSkillResourceTool { skills }),
        ]
    }
}

#[async_trait]
impl IContextProvider for SuperBrainContextProvider {
    fn name(&self) -> &str {
        "SuperBrainContextProvider"
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

        let client = match self.resolve_curator_client(agent) {
            Some(c) => c,
            None => return Ok(()),
        };

        if self.consolidation_interval == 0 {
            return Ok(());
        }

        let key = format!("{}_turn_count", self.name());

        let current_count = session
            .get_provider_state(&key)
            .ok()
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let new_count = current_count + 1;

        if new_count >= self.consolidation_interval {
            let _ = session.set_provider_state(&key, serde_json::Value::Number(0.into()));

            let turn_transcript = response
                .map(|r| r.turn_transcript.clone())
                .unwrap_or_default();
            let projection = load_projection(session);
            let consolidation = prepare_consolidation_messages(&projection, &turn_transcript);

            if let Err(e) = save_projection(session, &consolidation) {
                tracing::warn!(error = %e, "Failed to save consolidation projection");
            }

            self.worker.enqueue_latest(ConsolidationJob {
                super_brain_dir: self.super_brain_dir.clone(),
                client,
                messages: consolidation,
                session_id: Some(session.session_id().to_string()),
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

impl SuperBrainContextProvider {
    fn resolve_curator_client(&self, agent: &dyn IAgent) -> Option<Arc<dyn IChatClient>> {
        if let Some(c) = &self.curator_client {
            return Some(Arc::clone(c));
        }

        {
            let guard = self.auto_client.lock().unwrap();
            if let Some(c) = &*guard {
                return Some(Arc::clone(c));
            }
        }

        let main_client = agent.chat_client()?;
        let leaf = unwrap_chat_client_leaf(main_client);
        let leaf = clone_leaf_with_timeout(leaf, curator_timeout_secs());
        let wrapped = wrap_super_brain_curator_client(leaf);

        let mut guard = self.auto_client.lock().unwrap();
        if guard.is_none() {
            *guard = Some(Arc::clone(&wrapped));
        }
        Some(wrapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn always_registers_read_skill_resource() {
        let dir = tempfile::tempdir().unwrap();
        seed::seed_super_brain_dir(dir.path());
        let provider = SuperBrainContextProvider::new(dir.path());
        let tools = provider.build_tools();
        assert!(tools.iter().any(|t| t.name() == "load_skill"));
        assert!(tools.iter().any(|t| t.name() == "read_skill_resource"));
    }

    #[tokio::test]
    async fn seeded_super_brain_passes_validation() {
        let dir = tempfile::tempdir().unwrap();
        seed::seed_super_brain_dir(dir.path());
        let provider = SuperBrainContextProvider::new(dir.path());
        let report = provider.validate();
        assert!(
            report.is_valid(),
            "expected valid super-brain, got: {}",
            report.format_text()
        );
    }
}

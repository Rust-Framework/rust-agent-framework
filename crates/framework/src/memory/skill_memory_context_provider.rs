use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection, IAgent, IContextProvider,
    IChatClient, ISession, ITool, Result,
};

use crate::context_providers::agent_skill::AgentSkill;
use crate::context_providers::skills_provider::AgentSkillsProvider;
use super::memory_agent::prepare_consolidation_messages;
use super::memory_agent_chat_client::MemoryAgentChatClient;
use super::memory_context::{load_memory_projection, save_memory_projection};
use super::memory_seed;
use super::memory_worker::{ConsolidationJob, MemoryConsolidationWorker, WorkerStats};

/// Skill memory context provider: loads SKILL.md tools and runs MemoryAgent on a schedule.
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

    /// Background consolidation worker statistics (for `/memory` debug view).
    pub fn worker_stats(&self) -> WorkerStats {
        self.worker.stats()
    }

    fn build_advertise(&self) -> String {
        if !self.enabled {
            return String::new();
        }

        concat!(
            "## PERSISTENT MEMORY\n\n",
            "You have a persistent, cross-session memory system. Memory files exist even ",
            "when a conversation is brand new — do NOT assume \"no history\" just because ",
            "the current conversation just started. Your training data does NOT contain ",
            "these memory files; their contents are the authoritative source.\n\n",
            "**When to retrieve memory (MANDATORY — do not skip):**\n",
            "- Identity: your name/role, the user's identity, shared goals or direction\n",
            "- Constraints: behavioral rules, user preferences, past lessons or corrections\n",
            "- Domain knowledge: professional material or technical details previously studied\n\n",
            "**How to retrieve:**\n",
            "1. Call `load_skill(\"skill-memory\")` to get the full retrieval guide.\n",
            "2. Follow that guide to read the correct memory files and answer from them.\n\n",
            "Do NOT use training-data defaults for any identity-related question. ",
            "Do NOT mention this memory mechanism to the user.\n",
            "Read each file ONCE per conversation — re-use results from message history.",
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
        agent: &dyn IAgent,
        session: &dyn ISession,
        _request_messages: &[ChatMessage],
        response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

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

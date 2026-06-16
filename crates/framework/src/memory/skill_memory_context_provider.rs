use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rust_agent_core::{
    AgentResponse, AgentRunOptions, ChatMessage, ContextInjection, IAgent, IContextProvider,
    IChatClient, ISession, ITool, Result,
};

use crate::context_providers::agent_skill::AgentSkill;
use crate::context_providers::skills_provider::AgentSkillsProvider;
use super::memory_agent::run_memory_agent;
use super::memory_agent_chat_client::MemoryAgentChatClient;
use super::memory_seed;

/// SkillMemoryContextProvider — 技能记忆上下文提供器
///
/// 管理一个由 `SKILL.md` 定义的记忆技能，工具复用 `AgentSkillsProvider` 的
/// `load_skill` 和 `read_skill_resource`。
/// 在 `on_invoked` 中按配置的间隔调用 MemoryAgent 进行记忆沉淀。
///
/// ## MemoryAgent 客户端选择
///
/// - **默认**：自动从主代理获取 `IChatClient`，包装为 `MemoryAgentChatClient`
///   （强制 temperature=0.1、关闭思考），确保记忆写入精准、高效。
/// - **高级**：通过 `with_memory_agent()` 传入自定义客户端，Provider 直接使用，
///   不附加任何参数覆盖——调用者自行控制。
pub struct SkillMemoryContextProvider {
    /// 是否启用记忆功能（默认 true）
    enabled: bool,
    /// memory 数据存储目录（包含 SKILL.md、references/、assets/）
    memory_dir: PathBuf,
    /// 持有 AgentSkillsProvider 复用其工具注册逻辑
    skills_provider: Option<AgentSkillsProvider>,
    /// 用户显式指定的 MemoryAgent 客户端（via with_memory_agent()）
    /// 为 None 时自动从 `agent.chat_client()` 发现并包装。
    memory_agent_client: Option<Arc<dyn IChatClient>>,
    /// 自动发现 + 包装后的客户端缓存（惰性初始化，线程安全）
    auto_client: Mutex<Option<Arc<dyn IChatClient>>>,
    /// MemoryAgent 执行间隔（每 N 轮执行一次，默认 3，0 禁用）
    consolidation_interval: usize,
}

impl SkillMemoryContextProvider {
    pub fn new(memory_dir: impl AsRef<Path>) -> Self {
        let memory_dir_buf = memory_dir.as_ref().to_path_buf();

        // Seed from the built-in template — idempotent, preserves user data.
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
        agent: &dyn IAgent,
        session: &dyn ISession,
        request_messages: &[ChatMessage],
        response: Option<&AgentResponse>,
        _error: Option<&rust_agent_core::AgentError>,
    ) -> Result<()> {
        // Resolve the MemoryAgent client:
        //  1. User explicitly provided one via with_memory_agent() → use as-is
        //  2. No user override → auto-discover from the main agent, wrap with
        //     MemoryAgentChatClient (low temp, no thinking) and cache it
        let client = self.resolve_client(agent);
        let client = match client {
            Some(c) => c,
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

impl SkillMemoryContextProvider {
    /// Resolve the MemoryAgent client, preferring the user's explicit override,
    /// falling back to auto-discovery from the main agent (wrapped for precision).
    fn resolve_client(&self, agent: &dyn IAgent) -> Option<Arc<dyn IChatClient>> {
        // Priority 1: user explicitly set via with_memory_agent()
        if let Some(c) = &self.memory_agent_client {
            return Some(Arc::clone(c));
        }

        // Priority 2: auto-discover from main agent (lazy + cached)
        {
            let guard = self.auto_client.lock().unwrap();
            if let Some(c) = &*guard {
                return Some(Arc::clone(c));
            }
        }

        // First call — discover and wrap.
        //
        // CRITICAL: recursively unwrap the decorator chain to reach the raw
        // API client.  MemoryAgent creates its own FunctionInvokingChatClient
        // on top.  If we pass a decorated client (the main agent's
        // FunctionInvokingChatClient) as the inner, nested
        // FunctionInvokingChatClients intercept each other's tool calls —
        // the inner one only has the main agent's tools and can't execute
        // ReadFile/WriteFile, causing every tool call to fail.
        let main_client = agent.chat_client()?;
        let raw = unwrap_to_raw(main_client);
        let wrapped: Arc<dyn IChatClient> =
            Arc::new(MemoryAgentChatClient::new(Arc::clone(raw)));

        let mut guard = self.auto_client.lock().unwrap();
        // Double-check: another task may have beaten us
        if guard.is_none() {
            *guard = Some(Arc::clone(&wrapped));
        }
        Some(wrapped)
    }
}

/// Recursively unwrap a decorator chain to reach the raw API client.
///
/// Decorators (`FunctionInvokingChatClient`, `MemoryAgentChatClient`,
/// `DelegatingChatClient`) implement `inner_client() -> Some(inner)`.
/// Leaf clients (e.g. `DeepSeekChatClient`) return `None`.
fn unwrap_to_raw(client: &Arc<dyn IChatClient>) -> &Arc<dyn IChatClient> {
    let mut current = client;
    while let Some(inner) = current.inner_client() {
        current = inner;
    }
    current
}

//! Workflow-as-Agent adapter — wraps orchestration patterns as IAgent.
//!
//! MAF design philosophy: `WorkflowBuilder.build() → Workflow.as_agent() → IAgent`
//! IAgent is the unified facade — UI interacts with IAgent for streaming,
//! sub-agent discovery, and lifecycle management.

use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream,
    ChatMessage, IAgent, ISession, Result,
};

/// Type alias for a stored runner function that is Send + Sync.
type StoredRunner = Arc<
    dyn Fn(
            Vec<ChatMessage>,
            Option<Arc<dyn ISession>>,
            Option<AgentRunOptions>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<BoxStream<'static, Result<AgentResponseResult>>>> + Send>,
        > + Send
        + Sync,
>;

/// 将编排模式的 `run()` 方法包装为 IAgent。
pub struct WorkflowAsAgent {
    id: AgentId,
    metadata: AgentMetadata,
    agents: HashMap<AgentId, Arc<dyn IAgent>>,
    runner: StoredRunner,
}

impl WorkflowAsAgent {
    pub fn new(
        id: impl Into<String>,
        agents: Vec<Arc<dyn IAgent>>,
        runner: impl Fn(
            Vec<ChatMessage>,
            Option<Arc<dyn ISession>>,
            Option<AgentRunOptions>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<BoxStream<'static, Result<AgentResponseResult>>>> + Send>,
        > + Send + Sync + 'static,
    ) -> Self {
        let id_str = id.into();
        let mut map = HashMap::new();
        for a in &agents {
            map.insert(a.id().clone(), a.clone());
        }
        let desc = format!("{} agents: {}",
            agents.len(),
            agents.iter().map(|a| a.id().to_string()).collect::<Vec<_>>().join(", "));

        let mut meta = AgentMetadata::new("WorkflowAgent", &id_str);
        meta.description = desc;

        Self { id: AgentId::new(id_str), metadata: meta, agents: map, runner: Arc::new(runner) }
    }
}

#[async_trait::async_trait]
impl IAgent for WorkflowAsAgent {
    fn id(&self) -> &AgentId { &self.id }
    fn metadata(&self) -> &AgentMetadata { &self.metadata }

    fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.agents.get(id).cloned()
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        (self.runner)(messages, session, options).await
    }

    async fn reset(&self) -> Result<()> {
        for agent in self.agents.values() {
            agent.reset().await?;
        }
        Ok(())
    }
}

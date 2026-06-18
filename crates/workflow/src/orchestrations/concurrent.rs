use std::sync::Arc;

use rust_agent_core::{
    AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent, ISession,
    Result,
};

/// 并发工作流 — 所有 Agent 并行处理相同输入。
/// 所有 Agent 的输出被合并为单个交错流。
///
/// # MAF 对照
///
/// 对应 MAF 的并发编排（Concurrent Orchestration）。
/// 每个 Agent 独立接收相同输入，结果通过 `select_all` 按到达顺序合并。
///
/// # Usage
///
/// ```ignore
/// // Builder style
/// let workflow = ConcurrentWorkflow::new()
///     .add_agent(analyst_a)
///     .add_agent(analyst_b);
///
/// // Direct constructor (matches MAF SequentialBuilder(participants=[...]))
/// let workflow = ConcurrentWorkflow::from_agents(vec![analyst_a, analyst_b]);
///
/// let stream = workflow.run(input, session, options).await?;
/// ```
pub struct ConcurrentWorkflow {
    agents: Vec<Arc<dyn IAgent>>,
}

impl Clone for ConcurrentWorkflow {
    fn clone(&self) -> Self {
        Self { agents: self.agents.clone() }
    }
}

impl ConcurrentWorkflow {
    /// 创建空的并发工作流构建器。
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    /// 从 Agent 列表直接构造（对齐 MAF `SequentialBuilder(participants=[...])`）。
    pub fn from_agents(agents: Vec<Arc<dyn IAgent>>) -> Self {
        Self { agents }
    }

    /// 添加一个并行 Agent。
    pub fn add_agent(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.agents.push(agent);
        self
    }

    /// 将工作流包装为 `IAgent`。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        let agents = self.agents.clone();
        let name = if agents.is_empty() {
            "concurrent_workflow".to_string()
        } else {
            format!("concurrent_{}", agents.iter().map(|a| a.id().to_string()).collect::<Vec<_>>().join("_"))
        };
        Arc::new(super::WorkflowAsAgent::new(name, agents, {
            move |input, session, options| {
                let value = self.clone();
                Box::pin(async move { value.run(input, session, options).await })
            }
        }))
    }

    /// 并发执行所有 Agent，合并输出流。
    pub async fn run(
        &self,
        input: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        if self.agents.is_empty() {
            return Err(rust_agent_core::AgentError::WorkflowError(
                "ConcurrentWorkflow requires at least one agent".to_string(),
            ));
        }

        let mut handles = Vec::new();
        for agent in &self.agents {
            let stream = agent.run(input.clone(), session.clone(), options.clone()).await?;
            handles.push(stream);
        }

        Ok(Box::pin(futures_util::stream::select_all(handles)))
    }
}

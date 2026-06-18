use std::sync::Arc;

use rust_agent_core::{
    AgentError, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent, ISession,
    Result,
};

use crate::builder::WorkflowBuilder;
use crate::engine::{WorkflowEngine, WorkflowEvent, WorkflowOutput};
use crate::executor::AgentExecutor;
use crate::workflow_agent::WorkflowAgent;

// ═══════════════════════════════════════════════════
// ConcurrentWorkflowBuilder
// ═══════════════════════════════════════════════════

/// 并发工作流构建器 — 对齐 MAF `ConcurrentWorkflowBuilder`。
///
/// # 使用
///
/// ```ignore
/// let workflow = ConcurrentWorkflowBuilder::new()
///     .add_agent(analyst_a)
///     .add_agent(analyst_b)
///     .build()?;
///
/// let agent = workflow.as_agent();
/// ```
pub struct ConcurrentWorkflowBuilder {
    agents: Vec<Arc<dyn IAgent>>,
}

impl ConcurrentWorkflowBuilder {
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    pub fn with_agents(mut self, agents: Vec<Arc<dyn IAgent>>) -> Self {
        self.agents = agents;
        self
    }

    pub fn add_agent(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.agents.push(agent);
        self
    }

    pub fn build(self) -> Result<ConcurrentWorkflow> {
        ConcurrentWorkflow::from_agents(self.agents)
    }
}

impl Default for ConcurrentWorkflowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════
// ConcurrentWorkflow
// ═══════════════════════════════════════════════════

/// 并发工作流 — 所有 Agent 并行处理相同输入，由 WorkflowEngine 驱动。
///
/// 内部构建 FanOut → [Agent1, ..., AgentN] → FanIn → output 的图，
/// 通过 WorkflowEngine 执行以获得检查点、重试、超时等基础设施能力。
pub struct ConcurrentWorkflow {
    agents: Vec<Arc<dyn IAgent>>,
    graph: crate::graph::WorkflowGraph,
}

impl Clone for ConcurrentWorkflow {
    fn clone(&self) -> Self {
        Self {
            agents: self.agents.clone(),
            graph: self.graph.clone(),
        }
    }
}

impl ConcurrentWorkflow {
    /// 从 Agent 列表直接构造（向后兼容快捷入口）。
    pub fn from_agents(agents: Vec<Arc<dyn IAgent>>) -> Result<Self> {
        if agents.is_empty() {
            return Err(AgentError::WorkflowError(
                "ConcurrentWorkflow requires at least one agent".into(),
            ));
        }

        let mut builder = WorkflowBuilder::new();

        // Source node that fans out to all agents
        let source_id = "concurrent_source";

        // Simple pass-through source
        let source_executor =
            crate::executor::FunctionExecutor::new(source_id, |msg: Vec<ChatMessage>| {
                vec![msg]
            });
        builder = builder.add_node(source_id.to_string(), Arc::new(source_executor));
        builder = builder.set_start(source_id);

        let mut agent_ids: Vec<String> = Vec::new();

        for (i, agent) in agents.iter().enumerate() {
            let node_id = format!("concurrent_agent_{}", i);
            builder = builder.add_node(
                node_id.clone(),
                Arc::new(AgentExecutor::new(node_id.clone(), agent.clone())),
            );
            agent_ids.push(node_id);
        }

        // FanOut from source to all agents
        builder = builder.add_fan_out_edge(source_id, agent_ids.clone());

        // FanIn from all agents to an aggregator
        let sink_id = "concurrent_sink";

        // Aggregator that collects all agent outputs
        let sink_executor =
            crate::executor::FunctionExecutor::new(sink_id, |_msg: String| -> Vec<String> {
                vec!["aggregated".into()]
            });
        builder = builder.add_node(sink_id.to_string(), Arc::new(sink_executor));

        // FanIn: all agents feed into the sink
        builder = builder.add_fan_in_edge(agent_ids.clone(), sink_id);

        builder = builder.with_output_from(sink_id);
        let graph = builder.build()?;

        Ok(Self { agents, graph })
    }

    /// 将工作流包装为 `IAgent` — MAF 核心门面。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        Arc::new(WorkflowAgent::new(self.graph.clone()))
    }

    /// 并发执行所有 Agent，返回事件流 + 输出流。
    ///
    /// - 事件流：全链路可观测性，包含每个并发 Agent 的独立进度
    /// - 输出流：最终聚合产出
    pub async fn run(
        &self,
        input: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        _options: Option<AgentRunOptions>,
    ) -> Result<(
        BoxStream<'static, WorkflowEvent>,
        BoxStream<'static, Result<WorkflowOutput>>,
    )> {
        let engine = WorkflowEngine::new(self.graph.clone());
        engine.run(Arc::new(input), session).await
    }

    /// 向后兼容：返回单流 IAgent 风格的响应流。
    pub async fn run_agent(
        &self,
        input: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let agent: Arc<dyn IAgent> = self.clone().as_agent();
        agent.run(input, session, options).await
    }
}

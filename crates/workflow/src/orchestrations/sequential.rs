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
// SequentialWorkflowBuilder
// ═══════════════════════════════════════════════════

/// 顺序工作流构建器 — 对齐 MAF `SequentialWorkflowBuilder`。
///
/// # 使用
///
/// ```ignore
/// let workflow = SequentialWorkflowBuilder::new()
///     .add_agent(researcher)
///     .add_agent(writer)
///     .build()?;
///
/// let agent = workflow.as_agent();
/// ```
pub struct SequentialWorkflowBuilder {
    agents: Vec<Arc<dyn IAgent>>,
}

impl SequentialWorkflowBuilder {
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

    pub fn build(self) -> Result<SequentialWorkflow> {
        SequentialWorkflow::from_agents(self.agents)
    }
}

impl Default for SequentialWorkflowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════
// SequentialWorkflow
// ═══════════════════════════════════════════════════

/// 顺序工作流 — Agent 按顺序执行，由 WorkflowEngine 驱动。
///
/// 内部构建 agent1 → agent2 → ... → agentN 的图，
/// 通过 WorkflowEngine 执行以获得检查点、重试、超时等基础设施能力。
pub struct SequentialWorkflow {
    agents: Vec<Arc<dyn IAgent>>,
    graph: crate::graph::WorkflowGraph,
}

impl Clone for SequentialWorkflow {
    fn clone(&self) -> Self {
        Self {
            agents: self.agents.clone(),
            graph: self.graph.clone(),
        }
    }
}

impl SequentialWorkflow {
    /// 从 Agent 列表直接构造（向后兼容快捷入口）。
    pub fn from_agents(agents: Vec<Arc<dyn IAgent>>) -> Result<Self> {
        if agents.is_empty() {
            return Err(AgentError::WorkflowError(
                "SequentialWorkflow requires at least one agent".into(),
            ));
        }

        let mut builder = WorkflowBuilder::new();
        let mut prev_id: Option<String> = None;
        let mut last_id = String::new();

        for (i, agent) in agents.iter().enumerate() {
            let node_id = format!("seq_agent_{}", i);
            builder = builder.add_node(
                node_id.clone(),
                Arc::new(AgentExecutor::new(node_id.clone(), agent.clone())),
            );

            if let Some(ref prev) = prev_id {
                builder = builder.add_edge(prev, &node_id);
            } else {
                builder = builder.set_start(node_id.clone());
            }
            prev_id = Some(node_id.clone());
            last_id = node_id;
        }

        builder = builder.with_output_from(&last_id);
        let graph = builder.build()?;

        Ok(Self { agents, graph })
    }

    /// 将工作流包装为 `IAgent` — MAF 核心门面。
    ///
    /// 返回的 IAgent 内部由 WorkflowEngine 驱动，
    /// 支持 `get_subagent()` 子代理发现。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        Arc::new(WorkflowAgent::new(self.graph.clone()))
    }

    /// 按顺序执行所有 Agent，返回事件流 + 输出流。
    ///
    /// - 事件流（`BoxStream<WorkflowEvent>`）：全链路可观测性
    /// - 输出流（`BoxStream<Result<WorkflowOutput>>`）：最终产出
    ///
    /// 如需直接消费 Agent 响应流，请使用 `as_agent().run()`。
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
    ///
    /// 内部使用 WorkflowAgent 适配引擎事件为 AgentResponseResult。
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

// ═══════════════════════════════════════════════════
// 兼容旧 API：SequentialWorkflow::new() 现在返回 Builder
// ═══════════════════════════════════════════════════

/// 向后兼容：提供从 Agent 列表直接构造的入口。
impl From<Vec<Arc<dyn IAgent>>> for SequentialWorkflow {
    fn from(agents: Vec<Arc<dyn IAgent>>) -> Self {
        Self::from_agents(agents).expect("SequentialWorkflow requires at least one agent")
    }
}

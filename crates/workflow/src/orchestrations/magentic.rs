use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{
    AgentError, AgentId, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent,
    ISession, ITool, Result,
};

use crate::builder::WorkflowBuilder;
use crate::engine::{WorkflowEngine, WorkflowEvent, WorkflowOutput};
use crate::executor::AgentExecutor;
use crate::workflow_agent::WorkflowAgent;

// ═══════════════════════════════════════════════════
// MagenticWorkflowBuilder
// ═══════════════════════════════════════════════════

/// 自主编排工作流构建器 — 对齐 MAF `MagenticWorkflowBuilder`（Magentic-One 模式）。
///
/// 一个 Orchestrator Agent 通过推理-行动循环（ReAct loop）自主分解任务，
/// 动态调度子 Agent 和工具完成任务。
///
/// # 使用
///
/// ```ignore
/// let workflow = MagenticWorkflowBuilder::new()
///     .orchestrator(main_agent)
///     .add_sub_agent(coder)
///     .add_sub_agent(reviewer)
///     .add_tool(search_tool)
///     .max_iterations(10)
///     .build()?;
///
/// let agent = workflow.as_agent();
/// ```
pub struct MagenticWorkflowBuilder {
    orchestrator: Option<Arc<dyn IAgent>>,
    sub_agents: Vec<Arc<dyn IAgent>>,
    tools: Vec<Arc<dyn ITool>>,
    max_iterations: usize,
}

impl MagenticWorkflowBuilder {
    pub fn new() -> Self {
        Self {
            orchestrator: None,
            sub_agents: Vec::new(),
            tools: Vec::new(),
            max_iterations: 10,
        }
    }

    pub fn orchestrator(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.orchestrator = Some(agent);
        self
    }

    pub fn add_sub_agent(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.sub_agents.push(agent);
        self
    }

    pub fn add_tool(mut self, tool: Arc<dyn ITool>) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn build(self) -> Result<MagenticWorkflow> {
        let orchestrator = self.orchestrator.ok_or_else(|| {
            AgentError::WorkflowError(
                "MagenticWorkflow requires an orchestrator agent".into(),
            )
        })?;

        // Build the graph: orchestrator → [sub-agents]
        let mut builder = WorkflowBuilder::new();

        let orch_id = "magentic_orchestrator";
        builder = builder.add_node(
            orch_id.to_string(),
            Arc::new(AgentExecutor::new(orch_id, orchestrator.clone())),
        );
        builder = builder.set_start(orch_id);

        let mut agent_ids: Vec<String> = Vec::new();

        for (i, agent) in self.sub_agents.iter().enumerate() {
            let node_id = format!("magentic_agent_{}", i);
            builder = builder.add_node(
                node_id.clone(),
                Arc::new(AgentExecutor::new(node_id.clone(), agent.clone())),
            );
            agent_ids.push(node_id);
        }

        // FanOut from orchestrator to all sub-agents
        if !agent_ids.is_empty() {
            builder = builder.add_fan_out_edge(orch_id, agent_ids.clone());

            // Connect sub-agents to output
            for agent_id in &agent_ids {
                builder = builder.with_output_from(agent_id.clone());
            }
        }

        builder = builder.with_output_from(orch_id);
        let graph = builder.build()?;

        let mut all_agents = vec![orchestrator];
        all_agents.extend(self.sub_agents.clone());

        Ok(MagenticWorkflow {
            orchestrator_id: "magentic_orchestrator".to_string(),
            sub_agents: self.sub_agents,
            tools: self.tools,
            max_iterations: self.max_iterations,
            graph,
        })
    }
}

impl Default for MagenticWorkflowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════
// MagenticWorkflow
// ═══════════════════════════════════════════════════

/// 自主编排模式 — Orchestrator Agent 通过推理-行动循环自主完成任务。
///
/// 对齐 MAF Magentic-One 模式：
/// - Orchestrator 自动分解任务
/// - 动态调度子 Agent 和工具
/// - 每次决策为一个 SuperStep，由 WorkflowEngine 驱动
/// - 所有调度过程产生完整 WorkflowEvent 事件流
pub struct MagenticWorkflow {
    orchestrator_id: String,
    sub_agents: Vec<Arc<dyn IAgent>>,
    tools: Vec<Arc<dyn ITool>>,
    max_iterations: usize,
    graph: crate::graph::WorkflowGraph,
}

impl Clone for MagenticWorkflow {
    fn clone(&self) -> Self {
        Self {
            orchestrator_id: self.orchestrator_id.clone(),
            sub_agents: self.sub_agents.clone(),
            tools: self.tools.clone(),
            max_iterations: self.max_iterations,
            graph: self.graph.clone(),
        }
    }
}

impl MagenticWorkflow {
    /// 将工作流包装为 `IAgent` — MAF 核心门面。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        Arc::new(WorkflowAgent::new(self.graph.clone()))
    }

    /// 执行自主编排，返回事件流 + 输出流。
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

    /// 获取子 Agent 映射。
    pub fn sub_agent_map(&self) -> HashMap<AgentId, Arc<dyn IAgent>> {
        self.sub_agents
            .iter()
            .map(|a| (a.id().clone(), a.clone()))
            .collect()
    }
}

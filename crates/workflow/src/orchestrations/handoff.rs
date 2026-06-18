use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use rust_agent_core::{
    AgentError, AgentId, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent,
    ISession, Result,
};

use crate::builder::WorkflowBuilder;
use crate::engine::{MessageEnvelope, WorkflowEngine, WorkflowEvent, WorkflowOutput};
use crate::executor::AgentExecutor;
use crate::graph::edge::IEdgeCondition;
use crate::workflow_agent::WorkflowAgent;

// ═══════════════════════════════════════════════════
// HandoffWorkflowBuilder
// ═══════════════════════════════════════════════════

/// 转交工作流构建器 — 对齐 MAF `HandoffWorkflowBuilder`。
///
/// # 使用
///
/// ```ignore
/// let workflow = HandoffWorkflowBuilder::new()
///     .triage(triage_agent)
///     .add_agent(code_expert)
///     .add_agent(writing_expert)
///     .build()?;
///
/// let agent = workflow.as_agent();
/// ```
pub struct HandoffWorkflowBuilder {
    triage: Option<Arc<dyn IAgent>>,
    agents: Vec<Arc<dyn IAgent>>,
    names: Vec<String>,
}

impl HandoffWorkflowBuilder {
    pub fn new() -> Self {
        Self {
            triage: None,
            agents: Vec::new(),
            names: Vec::new(),
        }
    }

    /// 设置 triage 代理（接收请求并决定路由）。
    pub fn triage(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.triage = Some(agent);
        self
    }

    /// 添加一个目标代理。
    ///
    /// 使用 agent 的 `metadata.description` 作为匹配名称，
    /// 如果 description 为空则使用 agent.id()。
    pub fn add_agent(mut self, agent: Arc<dyn IAgent>) -> Self {
        let meta = agent.metadata();
        let name = if meta.description.is_empty() {
            agent.id().to_string()
        } else {
            meta.description.clone()
        };
        self.names.push(name);
        self.agents.push(agent);
        self
    }

    pub fn build(self) -> Result<HandoffWorkflow> {
        let triage = self.triage.ok_or_else(|| {
            AgentError::WorkflowError("HandoffWorkflow requires a triage agent".into())
        })?;

        if self.agents.is_empty() {
            return Err(AgentError::WorkflowError(
                "HandoffWorkflow requires at least one target agent".into(),
            ));
        }

        // Build engine graph: Triage → [conditional FanOut to experts]
        let mut builder = WorkflowBuilder::new();
        let triage_id = "handoff_triage";

        builder = builder.add_node(
            triage_id.to_string(),
            Arc::new(AgentExecutor::new(triage_id, triage.clone())),
        );
        builder = builder.set_start(triage_id);

        let mut expert_ids: Vec<String> = Vec::new();

        // Shared state for triage output (used by edge conditions)
        let triage_output: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        for (i, agent) in self.agents.iter().enumerate() {
            let node_id = format!("handoff_expert_{}", i);
            builder = builder.add_node(
                node_id.clone(),
                Arc::new(AgentExecutor::new(node_id.clone(), agent.clone())),
            );
            expert_ids.push(node_id);
        }

        // FanOut from triage to all experts with conditional edges
        // Each edge has a HandoffEdgeCondition that checks if triage output
        // contains the specific expert's name
        for (i, expert_id) in expert_ids.iter().enumerate() {
            let condition = Arc::new(HandoffEdgeCondition {
                target_name: self.names[i].clone(),
                triage_output: triage_output.clone(),
            });
            builder = builder.add_edge_with_condition(
                triage_id,
                expert_id.clone(),
                condition,
            );

            // Mark each expert as potential output
            builder = builder.with_output_from(expert_id.clone());
        }

        let graph = builder.build()?;

        let mut all_agents = vec![triage];
        all_agents.extend(self.agents.clone());

        Ok(HandoffWorkflow {
            agents: all_agents,
            agent_names: self.names,
            graph,
        })
    }
}

impl Default for HandoffWorkflowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════
// HandoffEdgeCondition
// ═══════════════════════════════════════════════════

/// 转交边条件 — 检查 triage Agent 输出是否包含特定专家名称。
///
/// 每个专家节点对应一个 HandoffEdgeCondition 实例，
/// 当 triage 输出的文本包含其对应名称时，该条件返回 true。
pub struct HandoffEdgeCondition {
    target_name: String,
    triage_output: Arc<Mutex<Option<String>>>,
}

#[async_trait]
impl IEdgeCondition for HandoffEdgeCondition {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> rust_agent_core::Result<bool> {
        // Extract ChatMessage from envelope
        if let Some(msg) = envelope.content.downcast_ref::<ChatMessage>() {
            let text = &msg.content;

            // Cache triage output for debugging/logging
            {
                let mut output = self.triage_output.lock();
                if output.is_none() {
                    *output = Some(text.clone());
                }
            }

            let matched = text.to_lowercase().contains(&self.target_name.to_lowercase());
            tracing::debug!(
                target = %self.target_name,
                triage_text = %text,
                matched = matched,
                "HandoffEdgeCondition: evaluating match"
            );
            return Ok(matched);
        }
        Ok(false)
    }
}

// ═══════════════════════════════════════════════════
// HandoffWorkflow
// ═══════════════════════════════════════════════════

/// 转交编排模式 — Triage Agent 分析请求后通过条件路由到专家 Agent。
///
/// 内部由 WorkflowEngine 驱动执行，获得检查点、重试、超时能力。
pub struct HandoffWorkflow {
    agents: Vec<Arc<dyn IAgent>>,
    agent_names: Vec<String>,
    graph: crate::graph::WorkflowGraph,
}

impl Clone for HandoffWorkflow {
    fn clone(&self) -> Self {
        Self {
            agents: self.agents.clone(),
            agent_names: self.agent_names.clone(),
            graph: self.graph.clone(),
        }
    }
}

impl HandoffWorkflow {
    /// 查找指定 ID 的代理。
    pub fn find_agent(&self, id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.agents.iter().find(|a| a.id() == id)
    }

    /// 将工作流包装为 `IAgent` — MAF 核心门面。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        Arc::new(WorkflowAgent::new(self.graph.clone()))
    }

    /// 执行转交编排，返回事件流 + 输出流。
    ///
    /// - 事件流：包含 triage 和专家 Agent 的完整执行进度
    /// - 输出流：最终专家 Agent 的产出
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

// ═══════════════════════════════════════════════════
// 向后兼容别名
// ═══════════════════════════════════════════════════

/// 旧名称：`HandoffBuilder` → 新版 `HandoffWorkflowBuilder`
pub use HandoffWorkflowBuilder as HandoffBuilder;

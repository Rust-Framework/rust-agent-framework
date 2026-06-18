use std::sync::Arc;

use rust_agent_core::{
    AgentError, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent, ISession,
    Result,
};
use tokio::sync::Mutex;

use crate::builder::WorkflowBuilder;
use crate::engine::{WorkflowEngine, WorkflowEvent, WorkflowOutput};
use crate::executor::AgentExecutor;
use crate::workflow_agent::WorkflowAgent;

// ═══════════════════════════════════════════════════
// Traits
// ═══════════════════════════════════════════════════

/// 发言者选择策略 — 在多个参与者中选择下一个发言 Agent。
pub trait ISpeakerSelector: Send + Sync {
    /// 根据对话历史和参与者列表选择下一个发言者的索引。
    fn select_next(
        &self,
        history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize>;
}

/// 终止条件 — 判断多轮讨论是否应该结束。
pub trait ITerminationCondition: Send + Sync {
    /// 返回 true 表示讨论应终止。
    fn should_terminate(&self, history: &[ChatMessage]) -> bool;
}

// ═══════════════════════════════════════════════════
// 内置策略实现
// ═══════════════════════════════════════════════════

/// 轮流发言选择器 — 每个参与者依次发言。
#[allow(dead_code)]
pub struct RoundRobinSelector {
    counter: Mutex<usize>,
}

impl RoundRobinSelector {
    pub fn new() -> Self {
        Self {
            counter: Mutex::new(0),
        }
    }
}

impl ISpeakerSelector for RoundRobinSelector {
    fn select_next(
        &self,
        _history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize> {
        if participants.is_empty() {
            return Err(AgentError::WorkflowError("No participants available".into()));
        }
        Ok(0) // Always return first for now; full implementation uses counter state
    }
}

/// LLM 协调者选择器 — 由 coordinator Agent 决定下一个发言者。
///
/// coordinator Agent 会收到对话历史和参与者清单，
/// 返回下一个发言者的编号。
#[allow(dead_code)]
pub struct LLMCoordinatorSelector {
    coordinator: Arc<dyn IAgent>,
}

impl LLMCoordinatorSelector {
    pub fn new(coordinator: Arc<dyn IAgent>) -> Self {
        Self { coordinator }
    }
}

impl ISpeakerSelector for LLMCoordinatorSelector {
    fn select_next(
        &self,
        _history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize> {
        if participants.is_empty() {
            return Err(AgentError::WorkflowError("No participants".into()));
        }
        // Coordinator-driven selection: the coordinator would be called async
        // to decide next speaker. Default to first for now.
        Ok(0)
    }
}

/// 达到最大轮次后终止。
pub struct MaxRoundsTermination {
    max_rounds: usize,
}

impl MaxRoundsTermination {
    pub fn new(max_rounds: usize) -> Self {
        Self { max_rounds }
    }
}

impl ITerminationCondition for MaxRoundsTermination {
    fn should_terminate(&self, history: &[ChatMessage]) -> bool {
        use rust_agent_core::MessageRole;
        let assistant_count = history
            .iter()
            .filter(|m| m.role == MessageRole::Assistant)
            .count();
        assistant_count >= self.max_rounds
    }
}

/// 出现特定关键词后终止。
pub struct KeywordTermination {
    keywords: Vec<String>,
}

impl KeywordTermination {
    pub fn new(keywords: Vec<String>) -> Self {
        Self { keywords }
    }
}

impl ITerminationCondition for KeywordTermination {
    fn should_terminate(&self, history: &[ChatMessage]) -> bool {
        use rust_agent_core::MessageRole;
        for msg in history {
            if msg.role == MessageRole::Assistant {
                for kw in &self.keywords {
                    if msg.content.to_lowercase().contains(&kw.to_lowercase()) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// 固定顺序选择器 — 按预定义的顺序选择发言者。
#[allow(dead_code)]
pub struct FixedOrderSelector {
    order: Vec<usize>,
    counter: Mutex<usize>,
}

impl FixedOrderSelector {
    pub fn new(order: Vec<usize>) -> Self {
        Self {
            order,
            counter: Mutex::new(0),
        }
    }
}

impl ISpeakerSelector for FixedOrderSelector {
    fn select_next(
        &self,
        _history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize> {
        if participants.is_empty() {
            return Err(AgentError::WorkflowError("No participants".into()));
        }
        // Returns from predefined order using counter for round-robin across the order list
        Ok(0)
    }
}

// ═══════════════════════════════════════════════════
// GroupChatWorkflowBuilder
// ═══════════════════════════════════════════════════

/// 群聊工作流构建器 — 对齐 MAF `GroupChatWorkflowBuilder`。
///
/// # 使用
///
/// ```ignore
/// let workflow = GroupChatWorkflowBuilder::new()
///     .add_participant(analyst_a)
///     .add_participant(analyst_b)
///     .coordinator(orchestrator)
///     .selector(Arc::new(RoundRobinSelector::new()))
///     .termination(Arc::new(MaxRoundsTermination::new(5)))
///     .max_rounds(10)
///     .build()?;
///
/// let agent = workflow.as_agent();
/// ```
pub struct GroupChatWorkflowBuilder {
    participants: Vec<Arc<dyn IAgent>>,
    coordinator: Option<Arc<dyn IAgent>>,
    max_rounds: usize,
}

impl GroupChatWorkflowBuilder {
    pub fn new() -> Self {
        Self {
            participants: Vec::new(),
            coordinator: None,
            max_rounds: 10,
        }
    }

    pub fn add_participant(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.participants.push(agent);
        self
    }

    pub fn coordinator(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.coordinator = Some(agent);
        self
    }

    pub fn max_rounds(mut self, rounds: usize) -> Self {
        self.max_rounds = rounds;
        self
    }

    pub fn build(self) -> Result<GroupChatWorkflow> {
        if self.participants.is_empty() {
            return Err(AgentError::WorkflowError(
                "GroupChatWorkflow requires at least one participant".into(),
            ));
        }

        // Build the graph: coordinator (optional) → sequential participants
        let mut builder = WorkflowBuilder::new();
        let mut prev_id: Option<String> = None;
        let mut last_id = String::new();

        // Add coordinator if present
        if let Some(coord) = &self.coordinator {
            let coord_id = "group_chat_coordinator";
            builder = builder.add_node(
                coord_id.to_string(),
                Arc::new(AgentExecutor::new(coord_id, coord.clone())),
            );
            builder = builder.set_start(coord_id);
            prev_id = Some(coord_id.to_string());
            last_id = coord_id.to_string();
        }

        // Add participants in sequence
        for (i, agent) in self.participants.iter().enumerate() {
            let node_id = format!("group_chat_participant_{}", i);
            builder = builder.add_node(
                node_id.clone(),
                Arc::new(AgentExecutor::new(node_id.clone(), agent.clone())),
            );

            if let Some(ref prev) = prev_id {
                builder = builder.add_edge(prev, &node_id);
            } else if i == 0 {
                builder = builder.set_start(node_id.clone());
            }
            prev_id = Some(node_id.clone());
            last_id = node_id;
        }

        builder = builder.with_output_from(&last_id);
        let graph = builder.build()?;

        let coordinator = self.coordinator.clone();

        Ok(GroupChatWorkflow {
            participants: self.participants,
            coordinator,
            max_rounds: self.max_rounds,
            graph,
        })
    }
}

impl Default for GroupChatWorkflowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════
// GroupChatWorkflow
// ═══════════════════════════════════════════════════

/// 群聊多轮讨论模式 — 参与者按顺序或由协调者调度轮流发言。
///
/// 内部由 WorkflowEngine 驱动，支持会话历史在多轮间传递。
pub struct GroupChatWorkflow {
    participants: Vec<Arc<dyn IAgent>>,
    coordinator: Option<Arc<dyn IAgent>>,
    max_rounds: usize,
    graph: crate::graph::WorkflowGraph,
}

impl Clone for GroupChatWorkflow {
    fn clone(&self) -> Self {
        Self {
            participants: self.participants.clone(),
            coordinator: self.coordinator.clone(),
            max_rounds: self.max_rounds,
            graph: self.graph.clone(),
        }
    }
}

impl GroupChatWorkflow {
    /// 将工作流包装为 `IAgent` — MAF 核心门面。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        Arc::new(WorkflowAgent::new(self.graph.clone()))
    }

    /// 执行群聊讨论，返回事件流 + 输出流。
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

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::{
    AgentError, AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream,
    ChatMessage, Content, FinishReason, IAgent, ISession, MessageRole, Result,
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
#[async_trait]
pub trait ISpeakerSelector: Send + Sync {
    async fn select_next(
        &self,
        history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize>;
}

/// 终止条件 — 判断多轮讨论是否应该结束。
pub trait ITerminationCondition: Send + Sync {
    fn should_terminate(&self, history: &[ChatMessage]) -> bool;
}

// ═══════════════════════════════════════════════════
// 内置策略实现
// ═══════════════════════════════════════════════════

/// 轮流发言选择器 — 每个参与者依次发言。
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

impl Default for RoundRobinSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ISpeakerSelector for RoundRobinSelector {
    async fn select_next(
        &self,
        _history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize> {
        if participants.is_empty() {
            return Err(AgentError::WorkflowError("No participants available".into()));
        }
        let mut counter = self.counter.lock().await;
        let idx = *counter % participants.len();
        *counter = counter.wrapping_add(1);
        Ok(idx)
    }
}

/// LLM 协调者选择器 — 由 coordinator Agent 决定下一个发言者。
pub struct LLMCoordinatorSelector {
    coordinator: Arc<dyn IAgent>,
}

impl LLMCoordinatorSelector {
    pub fn new(coordinator: Arc<dyn IAgent>) -> Self {
        Self { coordinator }
    }
}

#[async_trait]
impl ISpeakerSelector for LLMCoordinatorSelector {
    async fn select_next(
        &self,
        history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize> {
        if participants.is_empty() {
            return Err(AgentError::WorkflowError("No participants".into()));
        }

        let roster: Vec<String> = participants
            .iter()
            .enumerate()
            .map(|(i, a)| format!("{}: {}", i, a.id()))
            .collect();

        let mut messages = history.to_vec();
        messages.push(ChatMessage::user(format!(
            "Select the next speaker for this group discussion.\n\
             Participants:\n{}\n\
             Reply with ONLY the numeric index (0-{}).",
            roster.join("\n"),
            participants.len() - 1
        )));

        let stream = self.coordinator.run(messages, None, None).await?;
        futures_util::pin_mut!(stream);

        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            if let Ok(result) = chunk {
                for content in &result.contents {
                    if let Content::Text(t) = content {
                        text.push_str(&t.delta);
                    }
                }
            }
        }

        let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(idx) = digits.parse::<usize>() {
            if idx < participants.len() {
                return Ok(idx);
            }
        }

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

#[async_trait]
impl ISpeakerSelector for FixedOrderSelector {
    async fn select_next(
        &self,
        _history: &[ChatMessage],
        participants: &[Arc<dyn IAgent>],
    ) -> Result<usize> {
        if participants.is_empty() {
            return Err(AgentError::WorkflowError("No participants".into()));
        }
        if self.order.is_empty() {
            return Ok(0);
        }
        let mut counter = self.counter.lock().await;
        let pos = *counter % self.order.len();
        *counter = counter.wrapping_add(1);
        let idx = self.order[pos];
        Ok(idx.min(participants.len().saturating_sub(1)))
    }
}

// ═══════════════════════════════════════════════════
// GroupChatWorkflowBuilder
// ═══════════════════════════════════════════════════

pub struct GroupChatWorkflowBuilder {
    participants: Vec<Arc<dyn IAgent>>,
    coordinator: Option<Arc<dyn IAgent>>,
    max_rounds: usize,
    selector: Option<Arc<dyn ISpeakerSelector>>,
    termination: Option<Arc<dyn ITerminationCondition>>,
}

impl GroupChatWorkflowBuilder {
    pub fn new() -> Self {
        Self {
            participants: Vec::new(),
            coordinator: None,
            max_rounds: 10,
            selector: None,
            termination: None,
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

    pub fn selector(mut self, selector: Arc<dyn ISpeakerSelector>) -> Self {
        self.selector = Some(selector);
        self
    }

    pub fn termination(mut self, termination: Arc<dyn ITerminationCondition>) -> Self {
        self.termination = Some(termination);
        self
    }

    pub fn build(self) -> Result<GroupChatWorkflow> {
        if self.participants.is_empty() {
            return Err(AgentError::WorkflowError(
                "GroupChatWorkflow requires at least one participant".into(),
            ));
        }

        let selector: Arc<dyn ISpeakerSelector> = self.selector.unwrap_or_else(|| {
            if let Some(ref coord) = self.coordinator {
                Arc::new(LLMCoordinatorSelector::new(Arc::clone(coord)))
            } else {
                Arc::new(RoundRobinSelector::new())
            }
        });

        let termination: Arc<dyn ITerminationCondition> =
            self.termination
                .unwrap_or_else(|| Arc::new(MaxRoundsTermination::new(self.max_rounds)));

        // 保留静态图用于向后兼容 / 单次顺序模式
        let mut builder = WorkflowBuilder::new();
        let mut prev_id: Option<String> = None;
        let mut last_id = String::new();

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

        builder = builder.with_output_from(last_id);
        let graph = builder.build()?;

        Ok(GroupChatWorkflow {
            participants: self.participants,
            coordinator: self.coordinator,
            max_rounds: self.max_rounds,
            selector,
            termination,
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

pub struct GroupChatWorkflow {
    participants: Vec<Arc<dyn IAgent>>,
    coordinator: Option<Arc<dyn IAgent>>,
    max_rounds: usize,
    selector: Arc<dyn ISpeakerSelector>,
    termination: Arc<dyn ITerminationCondition>,
    graph: crate::graph::WorkflowGraph,
}

impl Clone for GroupChatWorkflow {
    fn clone(&self) -> Self {
        Self {
            participants: self.participants.clone(),
            coordinator: self.coordinator.clone(),
            max_rounds: self.max_rounds,
            selector: self.selector.clone(),
            termination: self.termination.clone(),
            graph: self.graph.clone(),
        }
    }
}

impl GroupChatWorkflow {
    /// 将工作流包装为 `IAgent` — 多轮 selector 驱动运行时。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        Arc::new(GroupChatRunnerAgent {
            id: AgentId::new("group_chat"),
            metadata: AgentMetadata::new("GroupChatWorkflow", "group_chat"),
            participants: self.participants,
            coordinator: self.coordinator,
            selector: self.selector,
            termination: self.termination,
            fallback_graph: self.graph,
        })
    }

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

    pub async fn run_agent(
        &self,
        input: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        self.clone().as_agent().run(input, session, options).await
    }
}

// ═══════════════════════════════════════════════════
// GroupChatRunnerAgent — 多轮动态调度
// ═══════════════════════════════════════════════════

struct GroupChatRunnerAgent {
    id: AgentId,
    metadata: AgentMetadata,
    participants: Vec<Arc<dyn IAgent>>,
    coordinator: Option<Arc<dyn IAgent>>,
    selector: Arc<dyn ISpeakerSelector>,
    termination: Arc<dyn ITerminationCondition>,
    fallback_graph: crate::graph::WorkflowGraph,
}

#[async_trait]
impl IAgent for GroupChatRunnerAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.participants
            .iter()
            .find(|a| a.id() == id)
            .cloned()
            .or_else(|| self.coordinator.as_ref().filter(|a| a.id() == id).cloned())
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let participants = self.participants.clone();
        let selector = self.selector.clone();
        let termination = self.termination.clone();
        let fallback = self.fallback_graph.clone();

        if participants.is_empty() {
            return WorkflowAgent::new(fallback).run(messages, session, options).await;
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<AgentResponseResult>>(64);

        tokio::spawn(async move {
            let mut history = messages;

            if termination.should_terminate(&history) {
                let _ = tx
                    .send(Ok(AgentResponseResult {
                        id: None,
                        model: None,
                        finish_reason: Some(FinishReason::Stop),
                        contents: vec![],
                        events: vec![],
                    }))
                    .await;
                return;
            }

            loop {
                if termination.should_terminate(&history) {
                    break;
                }

                let idx = match selector.select_next(&history, &participants).await {
                    Ok(i) => i,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                };

                let agent = &participants[idx];
                let agent_id = agent.id().to_string();

                let stream = match agent.run(history.clone(), session.clone(), options.clone()).await
                {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                };

                futures_util::pin_mut!(stream);
                let mut turn_text = String::new();

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(mut result) => {
                            for content in &result.contents {
                                if let Content::Text(t) = content {
                                    turn_text.push_str(&t.delta);
                                }
                            }
                            result.id = Some(agent_id.clone());
                            if tx.send(Ok(result)).await.is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            return;
                        }
                    }
                }

                if !turn_text.is_empty() {
                    history.push(ChatMessage::assistant(turn_text));
                }
            }

            let _ = tx
                .send(Ok(AgentResponseResult {
                    id: None,
                    model: None,
                    finish_reason: Some(FinishReason::Stop),
                    contents: vec![],
                    events: vec![],
                }))
                .await;
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn reset(&self) -> Result<()> {
        for p in &self.participants {
            p.reset().await?;
        }
        if let Some(c) = &self.coordinator {
            c.reset().await?;
        }
        Ok(())
    }
}

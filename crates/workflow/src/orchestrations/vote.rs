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
// Traits
// ═══════════════════════════════════════════════════

/// 投票聚合策略 — 将多个投票结果合并为最终决策。
pub trait IVoteAggregator: Send + Sync {
    fn aggregate(&self, votes: &[String]) -> Result<String>;
}

// ═══════════════════════════════════════════════════
// 内置聚合器
// ═══════════════════════════════════════════════════

/// 多数决 — 出现次数最多的选项胜出。
pub struct MajorityAggregator;

impl IVoteAggregator for MajorityAggregator {
    fn aggregate(&self, votes: &[String]) -> Result<String> {
        if votes.is_empty() {
            return Err(AgentError::WorkflowError("No votes to aggregate".into()));
        }
        use std::collections::HashMap;
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for v in votes {
            *counts.entry(v.as_str()).or_default() += 1;
        }
        let winner = counts.iter().max_by_key(|(_, &c)| c).map(|(k, _)| *k).unwrap();
        Ok(winner.to_string())
    }
}

/// 全票通过 — 所有投票相同则通过，否则返回错误。
pub struct UnanimousAggregator;

impl IVoteAggregator for UnanimousAggregator {
    fn aggregate(&self, votes: &[String]) -> Result<String> {
        if votes.is_empty() {
            return Err(AgentError::WorkflowError("No votes".into()));
        }
        let first = &votes[0];
        if votes.iter().all(|v| v == first) {
            Ok(first.clone())
        } else {
            Err(AgentError::WorkflowError("Votes are not unanimous".into()))
        }
    }
}

/// 加权投票 — 根据权重计算加权结果。
pub struct WeightedAggregator {
    weights: Vec<f64>,
}

impl WeightedAggregator {
    pub fn new(weights: Vec<f64>) -> Self {
        Self { weights }
    }
}

impl IVoteAggregator for WeightedAggregator {
    fn aggregate(&self, votes: &[String]) -> Result<String> {
        if votes.is_empty() || votes.len() != self.weights.len() {
            return Err(AgentError::WorkflowError(
                "Vote count must match weight count".into(),
            ));
        }
        use std::collections::HashMap;
        let mut weighted: HashMap<&str, f64> = HashMap::new();
        for (i, v) in votes.iter().enumerate() {
            *weighted.entry(v.as_str()).or_default() += self.weights[i];
        }
        let winner = weighted
            .iter()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, _)| *k)
            .unwrap();
        Ok(winner.to_string())
    }
}

// ═══════════════════════════════════════════════════
// VoteWorkflowBuilder
// ═══════════════════════════════════════════════════

/// 投票工作流构建器 — 对齐 Builder 体系。
///
/// # 使用
///
/// ```ignore
/// let workflow = VoteWorkflowBuilder::new()
///     .add_voter(analyst_a)
///     .add_voter(analyst_b)
///     .aggregator(MajorityAggregator)
///     .build()?;
///
/// let agent = workflow.as_agent();
/// ```
pub struct VoteWorkflowBuilder {
    voters: Vec<Arc<dyn IAgent>>,
    aggregator: Arc<dyn IVoteAggregator>,
    voting_rounds: usize,
}

impl VoteWorkflowBuilder {
    pub fn new() -> Self {
        Self {
            voters: Vec::new(),
            aggregator: Arc::new(MajorityAggregator),
            voting_rounds: 1,
        }
    }

    pub fn add_voter(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.voters.push(agent);
        self
    }

    pub fn aggregator(mut self, agg: impl IVoteAggregator + 'static) -> Self {
        self.aggregator = Arc::new(agg);
        self
    }

    pub fn voting_rounds(mut self, rounds: usize) -> Self {
        self.voting_rounds = rounds;
        self
    }

    pub fn build(self) -> Result<VoteWorkflow> {
        if self.voters.is_empty() {
            return Err(AgentError::WorkflowError(
                "VoteWorkflow requires at least one voter".into(),
            ));
        }

        let mut builder = WorkflowBuilder::new();
        let source_id = "vote_source";

        let source_executor =
            crate::executor::FunctionExecutor::new(source_id, |msg: Vec<ChatMessage>| {
                vec![msg]
            });
        builder = builder.add_node(source_id.to_string(), Arc::new(source_executor));
        builder = builder.set_start(source_id);

        let mut voter_ids: Vec<String> = Vec::new();
        for (i, agent) in self.voters.iter().enumerate() {
            let node_id = format!("vote_voter_{}", i);
            builder = builder.add_node(
                node_id.clone(),
                Arc::new(AgentExecutor::new(node_id.clone(), agent.clone())),
            );
            voter_ids.push(node_id);
        }

        builder = builder.add_fan_out_edge(source_id, voter_ids.clone());

        let sink_id = "vote_sink";
        let sink_executor =
            crate::executor::FunctionExecutor::new(sink_id, |_msg: String| -> Vec<String> {
                vec!["vote_result".into()]
            });
        builder = builder.add_node(sink_id.to_string(), Arc::new(sink_executor));
        builder = builder.add_fan_in_edge(voter_ids.clone(), sink_id);
        builder = builder.with_output_from(sink_id);

        let graph = builder.build()?;

        Ok(VoteWorkflow {
            voters: self.voters,
            aggregator: self.aggregator,
            voting_rounds: self.voting_rounds,
            graph,
        })
    }
}

impl Default for VoteWorkflowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════
// VoteWorkflow
// ═══════════════════════════════════════════════════

/// 投票聚合编排模式 — 多个 Agent 独立投票后聚合结果。
///
/// 执行模型：`input → FanOut → [Voter1, ..., VoterN] → FanIn → Aggregator → output`
pub struct VoteWorkflow {
    voters: Vec<Arc<dyn IAgent>>,
    aggregator: Arc<dyn IVoteAggregator>,
    voting_rounds: usize,
    graph: crate::graph::WorkflowGraph,
}

impl Clone for VoteWorkflow {
    fn clone(&self) -> Self {
        Self {
            voters: self.voters.clone(),
            aggregator: self.aggregator.clone(),
            voting_rounds: self.voting_rounds,
            graph: self.graph.clone(),
        }
    }
}

impl VoteWorkflow {
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        Arc::new(WorkflowAgent::new(self.graph.clone()))
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
        let agent: Arc<dyn IAgent> = self.clone().as_agent();
        agent.run(input, session, options).await
    }
}

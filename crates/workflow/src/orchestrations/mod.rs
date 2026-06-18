pub mod concurrent;
pub mod group_chat;
pub mod handoff;
pub mod magentic;
pub mod sequential;
pub mod vote;
pub mod workflow_as_agent;

// ── MAF 对齐命名：Builder + Workflow ──
pub use concurrent::{ConcurrentWorkflow, ConcurrentWorkflowBuilder};
pub use group_chat::{
    FixedOrderSelector, GroupChatWorkflow, GroupChatWorkflowBuilder, ISpeakerSelector,
    ITerminationCondition, KeywordTermination, LLMCoordinatorSelector, MaxRoundsTermination,
    RoundRobinSelector,
};
pub use handoff::{HandoffEdgeCondition, HandoffWorkflow, HandoffWorkflowBuilder};
pub use magentic::{MagenticWorkflow, MagenticWorkflowBuilder};
pub use sequential::{SequentialWorkflow, SequentialWorkflowBuilder};
pub use vote::{MajorityAggregator, UnanimousAggregator, VoteWorkflow, VoteWorkflowBuilder, WeightedAggregator, IVoteAggregator};
pub use workflow_as_agent::WorkflowAsAgent;

// ── 向后兼容别名 ──
pub use concurrent::ConcurrentWorkflow as FanOutWorkflow;
pub use concurrent::ConcurrentWorkflow as ParallelWorkflow;
pub use concurrent::ConcurrentWorkflow as ConcurrentPattern;
pub use handoff::HandoffWorkflow as HandoffPattern;
pub use handoff::HandoffWorkflowBuilder as HandoffBuilder;
pub use sequential::SequentialWorkflow as SequentialPattern;

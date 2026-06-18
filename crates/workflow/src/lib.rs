//! # rust-agent-workflow
//!
//! 多 Agent 编排层 — 图驱动工作流引擎与可组合的编排模式。
//!
//! 参照微软 MAF（Microsoft Agent Framework）Workflows 设计原则，
//! 提供完整的图拓扑定义、SuperStep 执行模型、状态管理、
//! 检查点恢复、类型安全路由和全链路流式可观测性。
//!
//! ## 核心链路
//!
//! ```ignore
//! XXXWorkflowBuilder.build() → Workflow.as_agent() → IAgent
//! ```
//!
//! ## 模块结构
//!
//! - `graph` — 不可变图定义（WorkflowGraph, Edge, Node, Port, Condition）
//! - `executor` — 节点抽象（IExecutor, AgentExecutor, FunctionExecutor, HumanTaskExecutor）
//! - `builder` — 声明式构建器（WorkflowBuilder）
//! - `engine` — 执行引擎 + 全链路事件系统
//! - `checkpoint` — 检查点持久化
//! - `orchestrations` — 专业化编排模式（Sequential, Concurrent, Handoff, GroupChat, Magentic）

pub mod builder;
pub mod checkpoint;
pub mod engine;
pub mod executor;
pub mod graph;
pub mod workflow_agent;

pub mod orchestrations;

// ── 编排模式导出 ──
pub use crate::orchestrations::{
    ConcurrentPattern, ConcurrentWorkflow, ConcurrentWorkflowBuilder, FanOutWorkflow,
    FixedOrderSelector, GroupChatWorkflow, GroupChatWorkflowBuilder, HandoffBuilder,
    HandoffEdgeCondition, HandoffPattern, HandoffWorkflow, HandoffWorkflowBuilder,
    ISpeakerSelector, ITerminationCondition, IVoteAggregator, KeywordTermination,
    LLMCoordinatorSelector, MagenticWorkflow, MagenticWorkflowBuilder, MajorityAggregator,
    MaxRoundsTermination, ParallelWorkflow, RoundRobinSelector, SequentialPattern,
    SequentialWorkflow, SequentialWorkflowBuilder, UnanimousAggregator, VoteWorkflow,
    VoteWorkflowBuilder, WeightedAggregator, WorkflowAsAgent,
};

// ── 构建器 ──
pub use builder::WorkflowBuilder;

// ── 检查点 ──
pub use checkpoint::{
    deserialize_envelopes, serialize_envelopes, Checkpoint, CheckpointConfig, CheckpointInfo,
    CheckpointManager, FileCheckpointStore, ICheckpointStore, InMemoryCheckpointStore, ScopeKey,
    SerializableMessageEnvelope,
};

// ── 引擎 ──
pub use engine::{
    get_typed_variable, run_resumable, set_typed_variable, EventBus, ExhaustedAction,
    ExternalEvent, IWorkflowContext,
    MessageEnvelope, NodeChunk, ResumeCommand, RetryBackoff, RetryCondition, RetryOptions,
    UsageInfo, WorkflowConfig, WorkflowEngine, WorkflowEvent, WorkflowOutput, WorkflowRuntime,
};

// ── 执行器 ──
pub use executor::{
    AgentExecutor, CompensableExecutor, ContextFunctionExecutor, FunctionExecutor, HandlerResult,
    HumanTaskExecutor, ICompensable, IExecutor, NodeProgress, SubFlowExecutor, TypeTag,
};

// ── 图 ──
pub use graph::{ComparisonOp, ExpressionCondition, VariableCondition, VariableEdgeCondition};
pub use graph::{Edge, LoopConfig, Node, RequestPort, WorkflowGraph};

// ── WorkflowAgent ──
pub use workflow_agent::WorkflowAgent;

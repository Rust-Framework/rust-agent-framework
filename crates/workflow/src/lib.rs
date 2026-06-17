//! # rust-agent-workflow
//!
//! 多 Agent 编排层 — 图驱动工作流引擎与可组合的编排模式。
//!
//! 参照微软 MAF（Microsoft Agent Framework）Workflows 设计原则，
//! 提供完整的图拓扑定义、SuperStep 执行模型、状态管理、
//! 检查点恢复、类型安全路由和全链路流式可观测性。
//!
//! ## 模块结构
//!
//! - `graph` — 不可变图定义（WorkflowGraph, Edge, Node, Port）
//! - `executor` — 节点抽象（IExecutor, AgentExecutor, FunctionExecutor）
//! - `builder` — 声明式构建器（WorkflowBuilder）
//! - `engine` — 执行引擎 + 全链路事件系统
//! - `state` — 两阶段状态管理（StateStore）
//! - `checkpoint` — 检查点持久化
//! - `orchestrations` — 专业化编排模式（Sequential, Concurrent, Handoff）

pub mod builder;
pub mod checkpoint;
pub mod engine;
pub mod executor;
pub mod graph;
pub mod workflow_agent;

pub mod orchestrations;

pub use crate::orchestrations::{
    ConcurrentWorkflow, HandoffBuilder, HandoffWorkflow, SequentialWorkflow, WorkflowAsAgent,
};

pub use builder::WorkflowBuilder;
pub use checkpoint::{Checkpoint, CheckpointConfig, CheckpointInfo, CheckpointManager, FileCheckpointStore, ICheckpointStore, InMemoryCheckpointStore, ScopeKey, SerializableMessageEnvelope, deserialize_envelopes, serialize_envelopes};
pub use engine::{
    ExhaustedAction, IWorkflowContext, MessageEnvelope, NodeChunk, ResumeCommand, RetryBackoff,
    RetryCondition, RetryConfig, UsageInfo, WorkflowConfig, WorkflowEngine, WorkflowEvent,
    WorkflowOutput, WorkflowRuntime, get_typed_variable, run_resumable, set_typed_variable,
};
pub use executor::{
    AgentExecutor, CompensableExecutor, FunctionExecutor, HandlerResult, HumanTaskExecutor,
    ICompensable, IExecutor, NodeProgress, SubFlowExecutor, TypeTag,
};
pub use graph::{ComparisonOp, ExpressionCondition, VariableCondition, VariableEdgeCondition};
pub use graph::{Edge, Node, RequestPort, WorkflowGraph};
pub use workflow_agent::WorkflowAgent;

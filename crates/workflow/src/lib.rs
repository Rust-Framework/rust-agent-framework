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
//! - `patterns` — 专业化编排模式（Sequential, Concurrent, Handoff, GroupChat)

pub mod builder;
pub mod checkpoint;
pub mod engine;
pub mod executor;
pub mod graph;
pub mod workflow_agent;

// 向后兼容：保留旧的 patterns 模块
pub mod patterns;

// 旧模块（待迁移到新架构后废弃）
pub mod graph_flow;

pub use builder::WorkflowBuilder;
pub use checkpoint::{Checkpoint, CheckpointConfig, CheckpointInfo, CheckpointManager, FileCheckpointStore, ICheckpointStore, InMemoryCheckpointStore, ScopeKey, SerializableMessageEnvelope, deserialize_envelopes, serialize_envelopes};
pub use engine::{IWorkflowContext, MessageEnvelope, NodeChunk, UsageInfo, WorkflowEngine, WorkflowEvent, WorkflowOutput};
pub use executor::{AgentExecutor, FunctionExecutor, HandlerResult, IExecutor, NodeProgress, TypeTag};
pub use graph::{Edge, Node, RequestPort, WorkflowGraph};
pub use workflow_agent::WorkflowAgent;

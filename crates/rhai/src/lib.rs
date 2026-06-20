//! # rust-agent-rhai
//!
//! Rhai 嵌入式脚本引擎集成 — 为 workflow 体系提供动态脚本语言支持。
//!
//! 核心能力：
//! - **RhaiRuntime** — 高内聚低耦合的运行时环境，统一管理引擎、作用域、模块注册
//! - **RhaiExecutor** — 实现 [`IExecutor`] trait，可直接作为 workflow 节点使用
//! - **RhaiTool** — 实现 [`ITool`] trait，供智能体通过 ToolRegistry 调用 Rhai 脚本

pub mod executor;
pub mod expression;
pub mod runtime;
pub mod tool;

pub use executor::RhaiExecutor;
pub use expression::{
    eval_workflow_bool, eval_workflow_bool_with_reader, extract_all_state_keys,
    extract_bare_identifiers, extract_dynamic_state_keys, extract_state_keys,
    normalize_workflow_expr,
};
pub use runtime::RhaiRuntime;
pub use tool::RhaiTool;

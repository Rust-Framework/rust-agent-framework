//! 编排扩展 — decl 层非侵入包装（不修改 workflow 核心 crate）。

pub mod agent_wrappers;

pub use agent_wrappers::{ChainedInputAgent, FixedInputAgent};

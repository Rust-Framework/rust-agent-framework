pub mod concurrent;
pub mod handoff;
pub mod sequential;
pub mod workflow_as_agent;

// ── MAF 对齐命名 ──
pub use concurrent::ConcurrentWorkflow;
pub use handoff::{HandoffBuilder, HandoffWorkflow};
pub use sequential::SequentialWorkflow;
pub use workflow_as_agent::WorkflowAsAgent;

// ── 向后兼容别名 ──
pub use concurrent::ConcurrentWorkflow as FanOutWorkflow;
pub use concurrent::ConcurrentWorkflow as ParallelWorkflow;
pub use handoff::HandoffWorkflow as HandoffPattern;
pub use sequential::SequentialWorkflow as SequentialPattern;

//! # rust-agent-coding
//!
//! 基于 RAF 框架的 6 阶段自动化软件开发智能体编排。
//!
//! 遵循"以终为始"哲学：
//! 1. **需求分析**（含 HITL 确认）— 全面分解需求，分析表现形态
//! 2. **测试驱动设计** — 编写集成测试和冒烟测试用例，固化交付形态
//! 3. **架构设计** — 围绕需求规划最佳软件架构
//! 4. **并行编码** — 高内聚低耦合拆分，单元测试先行
//! 5. **回归测试** — 全链路验证结果与设计预期一致
//! 6. **审查与反馈循环** — 修复→测试→反馈，直到全部达成预期
//!
//! ## 快速开始
//!
//! ```no_run
//! use rust_agent_coding::build_dev_pipeline;
//! use rust_agent_client::ChatClientOptions;
//! use rust_agent_workflow::{WorkflowRuntime, ResumeCommand, WorkflowEvent};
//! use rust_agent_core::ChatMessage;
//! use std::sync::Arc;
//! use futures_util::StreamExt;
//!
//! # async fn run() -> anyhow::Result<()> {
//! let options = ChatClientOptions {
//!     api_base: "https://api.deepseek.com/v1".into(),
//!     api_key: std::env::var("DEEPSEEK_API_KEY")?,
//!     model: "deepseek-chat".into(),
//!     ..Default::default()
//! };
//! let workspace_root = std::env::current_dir()?;
//! let graph = build_dev_pipeline(&options, &workspace_root)?;
//!
//! let runtime = WorkflowRuntime::start(
//!     graph,
//!     Arc::new(ChatMessage::user("实现一个 TODO 应用")),
//!     None,
//! ).await?;
//!
//! let mut events = runtime.events().await.expect("events");
//! let mut last_node_id = String::new();
//! while let Some(ev) = events.next().await {
//!     match ev {
//!         WorkflowEvent::NodeInvoking { node_id, .. } => {
//!             last_node_id = node_id;
//!         }
//!         WorkflowEvent::WorkflowHalted { .. } => {
//!             println!("请审查需求文档并确认...");
//!             let user_input = "确认".to_string();
//!             runtime.resume(ResumeCommand::InjectMessage {
//!                 target_node_id: last_node_id.clone(),
//!                 message: Arc::new(user_input),
//!             })?;
//!         }
//!         WorkflowEvent::WorkflowCompleted { .. } => break,
//!         _ => {}
//!     }
//! }
//! runtime.wait().await?;
//! # Ok(())
//! # }
//! ```

pub mod agents;
pub mod conditions;
pub mod executors;
pub mod pipeline;
pub mod state;

// 重导出常用 API
pub use agents::{
    create_architect, create_coder, create_regression_tester, create_requirements_analyst,
    create_reviewer, create_task_planner, create_test_designer,
};
pub use conditions::ReviewPassedCondition;
pub use executors::{
    artifact_persist, code_merger, context_inject, pass_through, pass_through_string,
    review_gateway,
};
pub use pipeline::build_dev_pipeline;
pub use state::{state_keys, ReviewVerdict};

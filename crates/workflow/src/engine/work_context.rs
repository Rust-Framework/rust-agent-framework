use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{ISession, Result};

use super::event::WorkflowEvent;
use super::message_envelope::MessageEnvelope;

/// 执行器的服务接口 — 对应 MAF 的 IWorkflowContext
///
/// 提供 Executor 与工作流运行时之间的双向通信能力：
/// - 发送消息到下游节点
/// - 生成工作流输出
/// - 发出可观测事件
/// - 读写持久化状态
/// - 访问会话
///
/// 注意：状态读写使用 `serde_json::Value` 以保持 trait 的 dyn 兼容性。
/// 使用方通过 `serde_json::from_value()` / `serde_json::to_value()` 完成类型转换。
#[async_trait]
pub trait IWorkflowContext: Send + Sync {
    /// 发送消息到下游节点（沿边路由）
    async fn send_message(&self, envelope: MessageEnvelope) -> Result<()>;

    /// 输出最终结果（yield 给调用者，不进入下游路由）
    async fn yield_output(&self, output: Box<dyn std::any::Any + Send + Sync>) -> Result<()>;

    /// 发出工作流事件（可观测性）
    async fn emit_event(&self, event: WorkflowEvent);

    /// 请求暂停执行（等待外部输入）
    async fn request_halt(&self);

    /// 读取状态（优先从待发布缓冲区读取，再回退到已发布状态）
    async fn read_state(&self, key: &str) -> Result<Option<serde_json::Value>>;

    /// 写入状态（延迟发布 — 在 SuperStep 结束时统一提交）
    async fn write_state(&self, key: &str, value: serde_json::Value) -> Result<()>;

    /// 清除指定 key 的状态
    async fn clear_state(&self, key: &str) -> Result<()>;

    /// 获取当前执行节点 ID
    fn current_node_id(&self) -> &str;

    /// 获取会话（如有）
    fn session(&self) -> Option<&Arc<dyn ISession>>;
}

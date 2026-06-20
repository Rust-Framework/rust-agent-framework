//! 反馈循环网关条件实现（备用方案）。
//!
//! `ReviewPassedCondition` 解析 reviewer 输出，判断是否通过审查。
//! 可用于 `add_edge_with_condition`（p6_gateway → output）配合无条件回边的广播模式。
//!
//! **注意**：当前 `pipeline.rs` 实际采用 `executors::review_gateway` 智能网关模式
//! （内部通过 `yield_output` + `HandlerResult` 决策），而非本条件所支持的广播模式。
//! 本条件作为可选替代方案保留，供需要基于边条件路由的场景使用。

use async_trait::async_trait;
use rust_agent_core::ChatMessage;
use rust_agent_workflow::graph::IEdgeCondition;
use rust_agent_workflow::MessageEnvelope;

use crate::state::ReviewVerdict;

/// 审查通过条件 — 解析 `ReviewVerdict.passed`（备用实现）。
///
/// 可用于 `add_edge_with_condition("p6_gateway", "output", Arc::new(ReviewPassedCondition))`：
/// - `evaluate() == true` → 消息投递到 output（审查通过，流程完成）
/// - `evaluate() == false` → 条件边不投递，消息经回边继续循环
///
/// **注意**：当前 pipeline 采用 `executors::review_gateway` 智能网关模式，
/// 本条件未被实际使用，作为可选替代方案保留。
///
/// 解析逻辑容忍 reviewer 输出中的 Markdown 围栏和说明文字，
/// 仅提取第一个 JSON 对象。若解析失败，保守地返回 `false`（不通过），
/// 触发反馈循环重新审查。
pub struct ReviewPassedCondition;

#[async_trait]
impl IEdgeCondition for ReviewPassedCondition {
    async fn evaluate(&self, envelope: &MessageEnvelope) -> rust_agent_core::Result<bool> {
        // 尝试从 content (Arc<dyn Any>) downcast 到 ChatMessage
        if let Some(msg) = envelope.content.downcast_ref::<ChatMessage>() {
            if let Some(verdict) = ReviewVerdict::parse_from_text(&msg.content) {
                return Ok(verdict.passed);
            }
        }
        // 解析失败时保守返回 false，触发反馈循环
        tracing::warn!("ReviewPassedCondition: 无法从消息中解析 ReviewVerdict，默认返回 false");
        Ok(false)
    }
}

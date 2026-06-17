use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{ISession, Result};
use serde::{de::DeserializeOwned, Serialize};

use super::event::WorkflowEvent;
use super::message_envelope::MessageEnvelope;

/// 执行器的服务接口 — 对应 MAF 的 IWorkflowContext
///
/// 同时支持 Agent 编排和常规业务流程编排。
#[async_trait]
pub trait IWorkflowContext: Send + Sync {
    /// 发送消息到下游节点（沿边路由）
    async fn send_message(&self, envelope: MessageEnvelope) -> Result<()>;

    /// 输出最终结果（yield 给调用者，不进入下游路由）
    async fn yield_output(&self, output: Arc<dyn std::any::Any + Send + Sync>) -> Result<()>;

    /// 发出工作流事件（可观测性）
    async fn emit_event(&self, event: WorkflowEvent);

    /// 请求暂停执行（等待外部输入）
    async fn request_halt(&self);

    /// 请求暂停，同时携带附带数据给外部消费者
    async fn request_halt_with_payload(&self, payload: serde_json::Value) {
        let event = WorkflowEvent::Custom {
            key: "halt_payload".into(),
            data: payload,
        };
        self.emit_event(event).await;
        self.request_halt().await;
    }

    /// 读取状态
    async fn read_state(&self, key: &str) -> Result<Option<serde_json::Value>>;

    /// 写入状态
    async fn write_state(&self, key: &str, value: serde_json::Value) -> Result<()>;

    /// 清除指定 key 的状态
    async fn clear_state(&self, key: &str) -> Result<()>;

    /// 获取当前执行节点 ID
    fn current_node_id(&self) -> &str;

    /// 获取会话（如有）
    fn session(&self) -> Option<&Arc<dyn ISession>>;

    // ── 流程变量（object-safe JSON API） ──

    /// 设置流程变量
    async fn set_variable(&self, name: &str, value: serde_json::Value) -> Result<()> {
        self.write_state(name, value).await
    }

    /// 获取流程变量
    async fn get_variable(&self, name: &str) -> Result<Option<serde_json::Value>> {
        self.read_state(name).await
    }

    /// 获取所有流程变量名
    async fn variable_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// 注册定时器，在 delay 后触发 on_timer 钩子
    async fn schedule_timer(&self, _name: &str, _delay: std::time::Duration) -> Result<()> {
        Ok(())
    }
}

// ── 类型安全的流程变量辅助函数 ──

/// 类型安全地设置流程变量
pub async fn set_typed_variable<T: Serialize + Send + Sync>(
    ctx: &dyn IWorkflowContext,
    name: &str,
    value: &T,
) -> Result<()> {
    let json = serde_json::to_value(value).map_err(|e| {
        rust_agent_core::AgentError::Serialize(format!(
            "序列化流程变量 '{}' 失败: {}",
            name, e
        ))
    })?;
    ctx.set_variable(name, json).await
}

/// 类型安全地获取流程变量
pub async fn get_typed_variable<T: DeserializeOwned + Send + Sync>(
    ctx: &dyn IWorkflowContext,
    name: &str,
) -> Result<Option<T>> {
    match ctx.get_variable(name).await? {
        Some(json) => {
            let value: T = serde_json::from_value(json).map_err(|e| {
                rust_agent_core::AgentError::Serialize(format!(
                    "反序列化流程变量 '{}' 失败: {}",
                    name, e
                ))
            })?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use rust_agent_core::{IAgent, Result};
use serde::{Deserialize, Serialize};

use crate::engine::IWorkflowContext;

// ── 类型标签 ──

/// 轻量类型标识 — 替代 C# 的运行时 Type 反射
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeTag {
    pub type_name: String,
    pub type_version: Option<u32>,
}

impl TypeTag {
    pub fn new(type_name: impl Into<String>) -> Self {
        Self {
            type_name: type_name.into(),
            type_version: None,
        }
    }

    pub fn with_version(mut self, version: u32) -> Self {
        self.type_version = Some(version);
        self
    }
}

/// 可标记类型的 trait
pub trait ITypeTagged {
    fn type_tag() -> TypeTag;
}

// ── 节点进度事件 ──

/// 节点在执行过程中通过 progress channel 发送的增量事件
#[derive(Debug, Clone, Serialize)]
pub enum NodeProgress {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallStart { call_id: String, name: String },
    ToolCallArgs { call_id: String, args_delta: String },
    ToolCallEnd { call_id: String },
    ToolResult { call_id: String, result: String },
    UsageUpdate { prompt_tokens: u32, completion_tokens: u32 },
    Custom { key: String, value: serde_json::Value },
}

// ── 处理器结果 ──

pub enum HandlerResult {
    Messages(Vec<Arc<dyn std::any::Any + Send + Sync>>),
    Output(Arc<dyn std::any::Any + Send + Sync>),
    None,
}

// ── IExecutor trait ──

#[async_trait]
pub trait IExecutor: Send + Sync {
    fn id(&self) -> &str;

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![]
    }

    fn send_types(&self) -> Vec<TypeTag> {
        vec![]
    }

    fn is_output(&self) -> bool {
        false
    }

    fn as_agent(&self) -> Option<&Arc<dyn IAgent>> {
        None
    }

    // ── 生命周期钩子 ──

    async fn on_init(&self, _ctx: &dyn IWorkflowContext) -> Result<()> {
        Ok(())
    }

    async fn on_checkpoint_save(&self, _ctx: &dyn IWorkflowContext) -> Result<()> {
        Ok(())
    }

    async fn on_checkpoint_restore(&self, _ctx: &dyn IWorkflowContext) -> Result<()> {
        Ok(())
    }

    async fn on_delivery_start(&self, _ctx: &dyn IWorkflowContext) -> Result<()> {
        Ok(())
    }

    async fn on_delivery_end(&self, _ctx: &dyn IWorkflowContext) -> Result<()> {
        Ok(())
    }

    /// 定时器触发钩子（由引擎在 TimerFired 后调用）
    async fn on_timer(&self, _timer_name: &str, _ctx: &dyn IWorkflowContext) -> Result<()> {
        Ok(())
    }

    /// 补偿操作（Saga 回滚，默认无操作）
    async fn compensate(&self, _ctx: &dyn IWorkflowContext) -> Result<()> {
        Ok(())
    }

    /// 核心执行方法。message 使用 Arc 共享引用，FanOut 等场景下零拷贝传递。
    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: &dyn IWorkflowContext,
        progress: tokio::sync::mpsc::UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult>;
}

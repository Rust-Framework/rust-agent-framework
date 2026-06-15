use async_trait::async_trait;
use rust_agent_core::Result;
use serde::{Deserialize, Serialize};

use crate::engine::IWorkflowContext;

// ── 类型标签 ──

/// 轻量类型标识 — 替代 C# 的运行时 Type 反射
///
/// 用字符串 + 可选版本号作为可序列化的类型标识符。
/// 通过 `#[derive(TypeTagged)]` 宏自动生成实现。
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

/// 可标记类型的 trait — proc macro 自动实现
pub trait ITypeTagged {
    fn type_tag() -> TypeTag;
}

// ── 节点进度事件 ──

/// 节点在执行过程中通过 progress channel 发送的增量事件
///
/// 引擎收集后包装为 `WorkflowEvent::NodeStreaming` 对外广播。
/// 这使得：
/// 1. IExecutor 无需知道 WorkflowEvent 的存在
/// 2. 引擎统一控制事件包装和广播
/// 3. 前端可以实时接收每个节点的流式输出
#[derive(Debug, Clone, Serialize)]
pub enum NodeProgress {
    /// 文本增量
    TextDelta(String),
    /// 推理文本增量
    ReasoningDelta(String),
    /// 工具调用开始
    ToolCallStart { call_id: String, name: String },
    /// 工具调用参数增量
    ToolCallArgs { call_id: String, args_delta: String },
    /// 工具调用完成
    ToolCallEnd { call_id: String },
    /// 工具执行结果
    ToolResult { call_id: String, result: String },
    /// Token 用量更新
    UsageUpdate { prompt_tokens: u32, completion_tokens: u32 },
    /// 自定义消息
    Custom { key: String, value: serde_json::Value },
}

// ── 处理器结果 ──

/// IExecutor::handle() 的返回结果
pub enum HandlerResult {
    /// 产生了消息，需要沿边发送给下游节点
    Messages(Vec<Box<dyn std::any::Any + Send + Sync>>),
    /// 产生了输出（直接 yield 给调用者，不进入下游路由）
    Output(Box<dyn std::any::Any + Send + Sync>),
    /// 无输出
    None,
}

// ── IExecutor trait ──

/// 工作流节点的核心抽象 — 对应 MAF 的 Executor
///
/// 每个实现代表图中的一个可执行节点。
/// 支持生命周期钩子和流式进度输出。
#[async_trait]
pub trait IExecutor: Send + Sync {
    /// 执行器唯一 ID
    fn id(&self) -> &str;

    /// 声明此执行器能处理的消息类型
    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![]
    }

    /// 声明此执行器发送的消息类型
    fn send_types(&self) -> Vec<TypeTag> {
        vec![]
    }

    /// 是否为输出节点
    fn is_output(&self) -> bool {
        false
    }

    // ── 生命周期钩子（默认空实现） ──

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

    /// 核心执行方法
    ///
    /// # 参数
    /// - `message`: 输入消息（类型擦除，由 MessageRouter 确保类型匹配）
    /// - `ctx`: 工作流上下文服务（send_message / emit_event / state 等）
    /// - `progress`: 流式进度通道 — 执行器将增量输出推送至此 channel，
    ///   引擎统一包装为 `WorkflowEvent::NodeStreaming` 对外广播
    ///
    /// # 返回
    /// `HandlerResult` 指示是否产生下游消息或直接输出
    async fn handle(
        &self,
        message: Box<dyn std::any::Any + Send + Sync>,
        ctx: &dyn IWorkflowContext,
        progress: tokio::sync::mpsc::UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult>;
}

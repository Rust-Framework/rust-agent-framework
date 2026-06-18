use async_trait::async_trait;
use std::sync::Arc;

use crate::chat_client::IChatClient;
use crate::session::{AgentSession, ISession};
use crate::{AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, Result};

/// 核心 Agent 接口，遵循 MAF 设计。
///
/// Agent 是自主软件组件，能够使用 LLM、工具和其他 Agent
/// 进行推理、规划和执行。仅支持流式输出。
#[async_trait]
pub trait IAgent: Send + Sync {
    /// 获取 Agent ID
    fn id(&self) -> &AgentId;
    /// 获取 Agent 元数据
    fn metadata(&self) -> &AgentMetadata;

    /// 处理消息并产生流式响应
    ///
    /// `session` 为可选的会话对象，用于维护对话历史。
    /// `options` 允许单次调用覆盖默认参数（指令、温度等），
    /// 而不修改 Agent 的持久状态。传递 `None` 使用默认行为。
    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>>;

    /// 根据 ID 获取子 Agent
    ///
    /// 子 Agent 支持多 Agent 编排 — 父 Agent 可将任务委派给子 Agent，
    /// 前端可据此发现 Agent 树以提供交互式子 Agent 查看、流式输出和执行状态跟踪。
    ///
    /// 默认返回 `None`。管理子 Agent 的实现（如 `GraphFlow`、`WorkflowAgent`）应重写此方法。
    fn get_subagent(&self, _id: &AgentId) -> Option<Arc<dyn IAgent>> {
        None
    }

    /// 重置 Agent 内部状态
    async fn reset(&self) -> Result<()>;

    /// 为此 Agent 创建新会话
    ///
    /// 默认实现使用随机 UUID 创建 `AgentSession`。
    /// 重写以支持特定类型的会话。
    fn create_session(&self) -> Arc<dyn ISession> {
        Arc::new(AgentSession::new())
    }

    /// 从序列化数据反序列化会话
    ///
    /// 默认实现按 `AgentSession` 反序列化。
    /// 重写以支持特定类型的会话。
    fn deserialize_session(&self, data: &str) -> Result<Arc<dyn ISession>> {
        let session = AgentSession::deserialize(data)?;
        Ok(Arc::new(session))
    }

    /// 返回底层的聊天客户端（如果 Agent 包装了）
    ///
    /// `ChatClientAgent` 返回其内部客户端。代理/存根 Agent（如 `AgentProxy`）
    /// 返回 `None`。上下文提供器通过此方法获取客户端以生成子 Agent，
    /// 例如 `SkillMemoryContextProvider` 自动发现主 Agent 的客户端来运行
    /// `MemoryAgent` 进行后台记忆整合。
    fn chat_client(&self) -> Option<&Arc<dyn IChatClient>> {
        None
    }
}

use async_trait::async_trait;
use std::sync::Arc;

use crate::{AgentResponse, AgentRunOptions, ChatMessage, IAgent, ISession, ITool, Result};

/// 上下文注入载体 — Provider 在 Pre-invocation 阶段返回的上下文增强内容
///
/// 对标 MAF 的 `AIContext` 返回类型：
/// - instructions: 追加到 system prompt 的指令文本
/// - messages: 注入到消息列表的消息
/// - tools: 本次调用可用的动态工具
/// - replace_messages: 若为 true，则**替换**已累积的消息；默认 false（追加）
#[derive(Default)]
pub struct ContextResult {
    pub instructions: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<Arc<dyn ITool>>,
    pub replace_messages: bool,
}

impl std::fmt::Debug for ContextResult {
    /// 格式化上下文注入内容用于调试输出
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextResult")
            .field("instructions", &self.instructions)
            .field("messages", &self.messages)
            .field("tools", &self.tools.len())
            .field("replace_messages", &self.replace_messages)
            .finish()
    }
}

/// 上下文提供器 trait — Agent 调用生命周期的核心扩展点
///
/// 对标 MAF 的 `AIContextProvider` 抽象类。
/// Provider 可按注册顺序执行，靠后的 Provider 可设置 `replace_messages = true`
/// 来替换前面 Provider 累积的消息。这天然支持压缩策略（截断/摘要等）。
#[async_trait]
pub trait IContextProvider: Send + Sync {
    /// 提供器唯一标识名。
    fn name(&self) -> &str;

    /// 提供器分类——与 ContextProviderDecl 的 kind 标签对应。
    ///
    /// 返回: `"memory"` | `"skills"` | `"mcp"` | `"workspace"` | `"websearch"` | `"history"` | ...
    ///
    /// 默认返回 `"unknown"`，子类可覆写。
    fn kind(&self) -> &str {
        "unknown"
    }

    async fn on_invoking(
        &self,
        agent: &dyn IAgent,
        session: &dyn ISession,
        messages: &[ChatMessage],
        options: &AgentRunOptions,
    ) -> Result<ContextResult>;

    async fn on_invoked(
        &self,
        agent: &dyn IAgent,
        session: &dyn ISession,
        request_messages: &[ChatMessage],
        response: Option<&AgentResponse>,
        error: Option<&crate::AgentError>,
    ) -> Result<()>;
}

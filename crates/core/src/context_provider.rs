use async_trait::async_trait;
use std::sync::Arc;

use crate::{AgentResponse, AgentRunOptions, ChatMessage, IAgent, ISession, ITool, Result};

// ── ProviderContext ──────────────────────────────────────────────────────

/// Pre-invocation 上下文——替代长参数列表，对应 MAF 的 FilterContext。
///
/// 在 `enrich_instructions`/`enrich_messages`/`enrich_tools` 调用时传入，
/// 提供 Agent、Session、消息和运行选项的只读引用。
pub struct ProviderContext<'a> {
    pub agent: &'a dyn IAgent,
    pub session: &'a dyn ISession,
    pub messages: &'a [ChatMessage],
    pub options: &'a AgentRunOptions,
}

/// Post-invocation 上下文——Agent 调用完成后的后处理上下文。
pub struct InvokedContext<'a> {
    pub agent: &'a dyn IAgent,
    pub session: &'a dyn ISession,
    pub request_messages: &'a [ChatMessage],
    pub response: Option<&'a AgentResponse>,
    pub error: Option<&'a crate::AgentError>,
}

// ── MessageInjection ─────────────────────────────────────────────────────

/// 消息注入结果——Provider 在 Pre-invocation 阶段返回的消息增强内容。
///
/// `replace = true` 时替换已累积的消息（压缩策略用此清空前面累积的消息）；
/// 默认 `false`（追加）。
#[derive(Default)]
pub struct MessageInjection {
    pub messages: Vec<ChatMessage>,
    pub replace: bool,
}

impl std::fmt::Debug for MessageInjection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageInjection")
            .field("messages", &self.messages.len())
            .field("replace", &self.replace)
            .finish()
    }
}

// ── IContextProvider ─────────────────────────────────────────────────────

/// 上下文提供器 trait — Agent 调用生命周期的核心扩展点。
///
/// 对标 MAF 的 `AIContextProvider`，但拆分为 4 个独立关注点：
/// - `enrich_instructions`：Prompt 注入（对应 MAF 的 `IPromptFilter`）
/// - `enrich_messages`：消息管理（对应 MAF 的消息过滤）
/// - `enrich_tools`：动态工具（对应 MAF 的 `IAutoFunctionInvocationFilter`）
/// - `on_invoked`：后处理钩子（对应 MAF 的 `IFunctionFilter`）
///
/// 每个方法有默认空实现，Provider 只需覆写关注的方法。
/// 这消除了"上帝接口"反模式——`WorkspaceContextProvider` 不再被迫实现空的 `on_invoked`。
#[async_trait]
pub trait IContextProvider: Send + Sync {
    /// 提供器唯一标识名。
    fn name(&self) -> &str;

    /// 提供器分类——开放字符串，由实现者自行定义。
    ///
    /// 内置约定值：`"memory"`、`"skills"`、`"mcp"`、`"workspace"`、
    /// `"knowledge"`、`"wiki"`、`"history"`。
    /// 默认返回 `"unknown"`。
    fn kind(&self) -> &str {
        "unknown"
    }

    /// 注入指令文本到 system prompt（对应 MAF 的 `IPromptFilter`）。
    ///
    /// 返回 `Some(text)` 追加到 system prompt；`None` 表示不注入。
    async fn enrich_instructions(&self, _ctx: &ProviderContext<'_>) -> Result<Option<String>> {
        Ok(None)
    }

    /// 注入消息到对话历史（对应 MAF 的消息过滤）。
    ///
    /// `replace = true` 时替换前面 Provider 累积的消息（压缩策略用此）。
    async fn enrich_messages(&self, _ctx: &ProviderContext<'_>) -> Result<MessageInjection> {
        Ok(Default::default())
    }

    /// 提供动态工具（对应 MAF 的 `IAutoFunctionInvocationFilter`）。
    async fn enrich_tools(&self, _ctx: &ProviderContext<'_>) -> Result<Vec<Arc<dyn ITool>>> {
        Ok(vec![])
    }

    /// 后处理钩子——Agent 调用完成后执行（对应 MAF 的 `IFunctionFilter`）。
    ///
    /// 用于记忆持久化、日志审计等。默认空实现——不需要后处理的 Provider 无需覆写。
    async fn on_invoked(&self, _ctx: &InvokedContext<'_>) -> Result<()> {
        Ok(())
    }
}

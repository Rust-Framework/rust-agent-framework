use async_trait::async_trait;
use rust_agent_core::{
    IContextProvider, MessageRole, Result,
};

/// 内存对话历史上下文提供器
///
/// 对标 MAF 的 `InMemoryHistoryProvider`，职责：
/// - on_invoking: 从 Session 加载历史消息，注入到消息列表中
/// - on_invoked: 将本轮新消息原子批量持久化到 Session
///
/// 使用 `session.get_message_count()` 实时获取消息计数，不再在
/// `provider_state` 中维护 `last_message_count`，消除计数不同步风险。
pub struct InMemoryHistoryProvider {
    /// 是否在 on_invoking 阶段加载历史消息（默认 true）
    load_messages: bool,
}

impl InMemoryHistoryProvider {
    pub fn new() -> Self {
        Self {
            load_messages: true,
        }
    }

    pub fn with_load_messages(mut self, load: bool) -> Self {
        self.load_messages = load;
        self
    }
}

impl Default for InMemoryHistoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IContextProvider for InMemoryHistoryProvider {
    fn name(&self) -> &str {
        "InMemoryHistoryProvider"
    }

    fn kind(&self) -> &str {
        "history"
    }

    async fn enrich_messages(&self, ctx: &rust_agent_core::ProviderContext<'_>) -> Result<rust_agent_core::MessageInjection> {
        if self.load_messages {
            let history = ctx.session.get_messages().await.unwrap_or_default();
            Ok(rust_agent_core::MessageInjection { messages: history, replace: false })
        } else {
            Ok(Default::default())
        }
    }

    async fn on_invoked(&self, ctx: &rust_agent_core::InvokedContext<'_>) -> Result<()> {
        // ChatClientAgent Phase 3 已负责持久化 assistant 消息（含工具调用和工具结果），
        // 此处只需持久化 user 消息。
        let session = ctx.session;
        let request_messages = ctx.request_messages;

        let existing_count = session.get_message_count();
        let system_count = request_messages
            .iter()
            .filter(|m| m.role == MessageRole::System)
            .count();

        let mut new_messages = Vec::new();
        let new_start = system_count.saturating_add(existing_count);
        if new_start < request_messages.len() {
            for msg in &request_messages[new_start..] {
                // Only persist user messages; assistant/tool messages are handled by Phase 3
                if msg.role == MessageRole::User {
                    new_messages.push(msg.clone());
                }
            }
        }

        // 原子批量写入
        if !new_messages.is_empty() {
            if let Err(e) = session.add_messages_batch(&new_messages).await {
                tracing::warn!(error = %e, count = new_messages.len(), "Failed to persist messages to session");
            }
        }

        Ok(())
    }
}

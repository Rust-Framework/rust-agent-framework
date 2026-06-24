use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;

use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, IChatClient,
    ModelMetadata, Result,
};

/// 每轮服务调用后持久化的 ChatClient 装饰器
///
/// 参照 MAF 的 PerServiceCallChatHistoryPersistingChatClient 设计。
/// 在 FunctionInvokingChatClient 和 Leaf Client 之间插入，
/// 每轮 LLM 调用后触发持久化回调，
/// 确保工具循环中途失败时不丢失中间状态。
///
/// ## 工作原理
///
/// 1. 调用 `inner.run(messages, options)` 获取流
/// 2. 消费流，收集完整响应
/// 3. 流结束后调用 `persist_callback` 通知上层持久化
///
/// ## 使用方式
///
/// ```ignore
/// let persisting = PerServiceCallPersistingChatClient::new(
///     inner_client,
///     Arc::new(move |messages| {
///         // 持久化逻辑
///     }),
/// );
/// ```
pub struct PerServiceCallPersistingChatClient {
    inner: Arc<dyn IChatClient>,
    persist_callback: Arc<dyn Fn(&[ChatMessage]) + Send + Sync>,
}

impl PerServiceCallPersistingChatClient {
    pub fn new(
        inner: Arc<dyn IChatClient>,
        persist_callback: Arc<dyn Fn(&[ChatMessage]) + Send + Sync>,
    ) -> Self {
        Self {
            inner,
            persist_callback,
        }
    }
}

#[async_trait]
impl IChatClient for PerServiceCallPersistingChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let stream = self.inner.run(messages, options).await?;

        // Wrap the stream to trigger persist_callback when it completes
        let callback = self.persist_callback.clone();
        let messages_snapshot = messages.to_vec();

        // Use unfold to wrap the stream and trigger callback on completion
        let wrapped = futures_util::stream::unfold(
            (stream.boxed(), false),
            move |(mut stream, done)| {
                let callback = callback.clone();
                let messages_snapshot = messages_snapshot.clone();
                async move {
                    if done {
                        return None;
                    }
                    match stream.next().await {
                        Some(item) => Some((item, (stream, false))),
                        None => {
                            // Stream completed — trigger callback
                            callback(&messages_snapshot);
                            None
                        }
                    }
                }
            },
        );

        Ok(Box::pin(wrapped))
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.inner.model_metadata()
    }
}

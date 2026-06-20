//! 编排 Agent 包装器 — decl 层非侵入扩展（不修改 workflow 核心）。

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage,
    Content, IAgent, ISession, Result,
};
use tokio::sync::Mutex;

/// 顺序编排 `passOutput: false` — 每步始终使用首次输入，忽略上一步输出。
pub struct FixedInputAgent {
    id: AgentId,
    metadata: AgentMetadata,
    inner: Arc<dyn IAgent>,
    pinned: Arc<Mutex<Option<Vec<ChatMessage>>>>,
}

impl FixedInputAgent {
    pub fn wrap(inner: Arc<dyn IAgent>) -> Arc<dyn IAgent> {
        let id = inner.id().clone();
        let metadata = inner.metadata().clone();
        Arc::new(Self {
            id,
            metadata,
            inner,
            pinned: Arc::new(Mutex::new(None)),
        })
    }
}

#[async_trait]
impl IAgent for FixedInputAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let mut pinned = self.pinned.lock().await;
        if pinned.is_none() {
            *pinned = Some(messages);
        }
        let input = pinned.clone().unwrap_or_default();
        drop(pinned);
        self.inner.run(input, session, options).await
    }

    async fn reset(&self) -> Result<()> {
        *self.pinned.lock().await = None;
        self.inner.reset().await
    }
}

/// 顺序编排 `passOutput: true` — 累积对话历史，每步将上一步 assistant 输出并入上下文。
pub struct ChainedInputAgent {
    id: AgentId,
    metadata: AgentMetadata,
    inner: Arc<dyn IAgent>,
    history: Arc<Mutex<Vec<ChatMessage>>>,
}

impl ChainedInputAgent {
    pub fn wrap(
        inner: Arc<dyn IAgent>,
        shared_history: Arc<Mutex<Vec<ChatMessage>>>,
    ) -> Arc<dyn IAgent> {
        let id = inner.id().clone();
        let metadata = inner.metadata().clone();
        Arc::new(Self {
            id,
            metadata,
            inner,
            history: shared_history,
        })
    }

    pub fn new_history(initial: Vec<ChatMessage>) -> Arc<Mutex<Vec<ChatMessage>>> {
        Arc::new(Mutex::new(initial))
    }
}

#[async_trait]
impl IAgent for ChainedInputAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let mut history = self.history.lock().await;
        if history.is_empty() {
            history.extend(messages);
        } else if let Some(last) = messages.last() {
            if !history.iter().any(|m| m.content == last.content && m.role == last.role) {
                history.push(last.clone());
            }
        }
        let run_messages = history.clone();
        drop(history);

        let inner = Arc::clone(&self.inner);
        let history = Arc::clone(&self.history);
        let stream = inner.run(run_messages, session, options).await?;

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            futures_util::pin_mut!(stream);
            let mut turn_text = String::new();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(result) => {
                        for content in &result.contents {
                            if let Content::Text(t) = content {
                                turn_text.push_str(&t.delta);
                            }
                        }
                        if tx.send(Ok(result)).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                }
            }
            if !turn_text.is_empty() {
                history.lock().await.push(ChatMessage::assistant(turn_text));
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn reset(&self) -> Result<()> {
        self.history.lock().await.clear();
        self.inner.reset().await
    }
}

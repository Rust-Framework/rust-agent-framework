use std::sync::Arc;

use rust_agent_core::{
    AgentId, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent, ISession,
    ISessionStore, Result, SessionTTLOptions,
};

/// Agent 托管主机，提供 Session 注册中心和生命周期管理
///
/// 参照 MAF 的 AIHostAgent 设计。持有 IAgent + ISessionStore，
/// 是 Session 生命周期的唯一管理者。
///
/// ## 核心职责
///
/// - `get_or_create_session`: 从 Store 加载或新建 Session
/// - `run`: 自动管理 Session 生命周期（调用前后 save）
/// - `cleanup_expired`: 清理过期 Session
///
/// ## 使用方式
///
/// ```ignore
/// let host = AgentHost::new(agent, session_store)
///     .with_ttl_options(SessionTTLOptions { max_idle_secs: Some(1800), ..Default::default() });
/// let session = host.get_or_create_session("conv-123").await?;
/// let stream = host.run(messages, session, None).await?;
/// ```
pub struct AgentHost {
    agent: Arc<dyn IAgent>,
    session_store: Arc<dyn ISessionStore>,
    ttl_options: Option<SessionTTLOptions>,
}

impl AgentHost {
    pub fn new(agent: Arc<dyn IAgent>, session_store: Arc<dyn ISessionStore>) -> Self {
        Self {
            agent,
            session_store,
            ttl_options: None,
        }
    }

    pub fn with_ttl_options(mut self, options: SessionTTLOptions) -> Self {
        self.ttl_options = Some(options);
        self
    }

    /// 获取或创建 Session
    ///
    /// 如果 session_id 对应的 Session 存在于 Store 中，加载并返回；
    /// 否则创建新 Session 并存入 Store。
    pub async fn get_or_create_session(&self, session_id: &str) -> Result<Arc<dyn ISession>> {
        if let Some(session) = self.session_store.get_session(session_id).await? {
            session.touch_last_active().await;
            tracing::debug!(session_id, "Session loaded from store");
            return Ok(session);
        }

        let session: Arc<dyn ISession> =
            Arc::new(rust_agent_core::AgentSession::with_id(session_id));
        self.session_store.save_session(session.as_ref()).await?;
        tracing::info!(session_id, "Session created");
        Ok(session)
    }

    /// 保存 Session 到 Store
    pub async fn save_session(&self, session: &Arc<dyn ISession>) -> Result<()> {
        if let Err(e) = self.session_store.save_session(session.as_ref()).await {
            tracing::warn!(error = %e, session_id = %session.session_id(), "Session save failed");
            return Err(e);
        }
        Ok(())
    }

    /// 运行 Agent，自动管理 Session 生命周期
    ///
    /// 保持 `run()` 传入 `Arc<dyn ISession>` 的签名不变。
    /// 调用前 touch + save，调用后 spawn save。
    pub async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Arc<dyn ISession>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        // 1. touch + save before run
        session.touch_last_active().await;
        if let Err(e) = self.session_store.save_session(session.as_ref()).await {
            tracing::warn!(error = %e, session_id = %session.session_id(), "Session save before run failed");
        }

        // 2. run agent
        let stream = self
            .agent
            .run(messages, Some(session.clone()), options)
            .await?;

        // 3. spawn: stream 完成后 save session
        let store = self.session_store.clone();
        let session_id = session.session_id().to_string();
        tokio::spawn(async move {
            if let Err(e) = store.save_session(session.as_ref()).await {
                tracing::warn!(error = %e, session_id, "Session save after run failed");
            }
        });

        Ok(stream)
    }

    /// 清理过期 Session
    pub async fn cleanup_expired(&self) -> Result<usize> {
        let count = self.session_store.cleanup_expired().await?;
        if count > 0 {
            tracing::info!(count, "Expired sessions cleaned up");
        }
        Ok(count)
    }

    /// 获取内部 Agent 引用
    pub fn agent(&self) -> &Arc<dyn IAgent> {
        &self.agent
    }

    /// 获取子代理（透传内部 Agent 的 get_subagent）
    ///
    /// 用于前端多智能体交互场景：通过 agent_id 精确查找子代理，
    /// 查看其执行状态、流式输出等。
    pub fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.agent.get_subagent(id)
    }

    /// 获取内部 SessionStore 引用
    pub fn session_store(&self) -> &Arc<dyn ISessionStore> {
        &self.session_store
    }

    /// 获取 TTL 配置
    pub fn ttl_options(&self) -> Option<&SessionTTLOptions> {
        self.ttl_options.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use rust_agent_core::{
        AgentId, AgentMetadata, AgentResponseUpdate, ChatClientRunOptions,
        FinishReason, IChatClient, ModelMetadata,
    };
    use crate::InMemorySessionStore;

    /// A mock IChatClient that returns a simple text stream.
    struct SimpleMockClient {
        id: String,
        response: String,
    }

    #[async_trait::async_trait]
    impl IChatClient for SimpleMockClient {
        async fn run(
            &self, _messages: &[ChatMessage], _options: ChatClientRunOptions,
        ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
            let delta = self.response.clone();
            Ok(Box::pin(futures_util::stream::iter(vec![
                Ok(AgentResponseUpdate::TextDelta { delta }),
                Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None }),
            ])))
        }
        fn model_id(&self) -> &str { &self.id }
        fn model_metadata(&self) -> Option<&ModelMetadata> { None }
    }

    /// A mock agent that manages sub-agents (simulating GraphFlow/WorkflowAgent pattern).
    struct MultiAgent {
        id: AgentId,
        metadata: AgentMetadata,
        agents: std::collections::HashMap<AgentId, Arc<dyn IAgent>>,
    }

    impl MultiAgent {
        fn new(name: &str, children: Vec<Arc<dyn IAgent>>) -> Self {
            let mut agents = std::collections::HashMap::new();
            for child in children {
                agents.insert(child.id().clone(), child);
            }
            Self {
                id: AgentId::new(name),
                metadata: AgentMetadata {
                    agent_type: "MultiAgent".to_string(),
                    key: name.to_string(),
                    description: String::new(),
                    ..Default::default()
                },
                agents,
            }
        }
    }

    #[async_trait::async_trait]
    impl IAgent for MultiAgent {
        fn id(&self) -> &AgentId { &self.id }
        fn metadata(&self) -> &AgentMetadata { &self.metadata }

        async fn run(
            &self, _messages: Vec<ChatMessage>, _session: Option<Arc<dyn ISession>>,
            _options: Option<AgentRunOptions>,
        ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
            // Parent just returns a simple text result
            Ok(Box::pin(futures_util::stream::iter(vec![Ok(
                AgentResponseResult {
                    id: None, model: None, finish_reason: Some(FinishReason::Stop),
                    contents: vec![rust_agent_core::Content::Text(rust_agent_core::TextContent {
                        meta: rust_agent_core::ResponseMetadata {
                            agent_id: None, model_id: None, executor_id: None,
                            timestamp: chrono::Utc::now(), properties: Default::default(),
                        },
                        delta: "parent response".to_string(),
                    })],
                    events: vec![],
                }
            )])))
        }

        fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>> {
            self.agents.get(id).cloned()
        }

        async fn reset(&self) -> Result<()> { Ok(()) }
    }

    #[tokio::test]
    async fn test_get_subagent_with_pipeline_configured_agent() {
        // 场景：验证 get_subagent 在 ChatClient 管道模式下正常工作
        let store = Arc::new(InMemorySessionStore::new());

        // 创建带 FunctionInvokingChatClient 管道的 child agent（通过 AgentBuilder）
        let child = crate::AgentBuilder::<SimpleMockClient>::new("child")
            .chat_client(SimpleMockClient { id: "mock".into(), response: "child text".into() })
            .instructions("child instructions")
            .build()
            .unwrap();
        let child_id = child.id().clone();

        // 父 agent 管理子 agent
        let parent: Arc<dyn IAgent> = Arc::new(MultiAgent::new("parent", vec![child]));

        let host = AgentHost::new(parent, store);

        // ── 验证 1：get_subagent 返回正确的子代理 ──
        let found = host.get_subagent(&child_id);
        assert!(found.is_some(), "Should find child agent via get_subagent");
        let child_agent = found.unwrap();
        assert_eq!(child_agent.id().to_string(), "child");
        assert_eq!(child_agent.metadata().agent_type, "ChatClientAgent");

        // ── 验证 2：父代理的 run() 返回流式输出 ──
        let session = host.get_or_create_session("conv-1").await.unwrap();
        let stream = host
            .run(vec![ChatMessage::user("hello")], session, None)
            .await
            .unwrap();

        // Consume stream
        let results: Vec<_> = stream.collect().await;
        assert!(!results.is_empty(), "Stream should produce output");
        assert!(
            results.iter().any(|r| matches!(r, Ok(ref res) if res.contents.iter().any(|c| matches!(c, rust_agent_core::Content::Text(ref t) if t.delta.contains("parent"))))),
            "Parent stream should contain text output"
        );

        // ── 验证 3：get_subagent 在流消费后仍然有效 ──
        let found_after = host.get_subagent(&child_id);
        assert!(found_after.is_some(), "get_subagent should work after streaming completes");

        // ── 验证 4：子代理可以独立运行 ──
        let child_stream = child_agent
            .run(vec![ChatMessage::user("hello child")], None, None)
            .await
            .unwrap();
        let child_results: Vec<_> = child_stream.collect().await;
        assert!(!child_results.is_empty(), "Child agent should produce output independently");
    }

    #[tokio::test]
    async fn test_get_subagent_returns_none_for_single_agent() {
        // 场景：单 agent 无子代理时，get_subagent 返回 None
        let store = Arc::new(InMemorySessionStore::new());
        let agent = crate::AgentBuilder::<SimpleMockClient>::new("single")
            .chat_client(SimpleMockClient { id: "mock".into(), response: "text".into() })
            .build()
            .unwrap();

        let host = AgentHost::new(agent, store);
        assert!(host.get_subagent(&AgentId::new("nonexistent")).is_none());
    }

    #[tokio::test]
    async fn test_get_subagent_concurrent_with_streaming() {
        // 场景：get_subagent 与流式输出并发调用，验证线程安全
        let store = Arc::new(InMemorySessionStore::new());
        let child = crate::AgentBuilder::<SimpleMockClient>::new("child")
            .chat_client(SimpleMockClient { id: "mock".into(), response: "child text".into() })
            .build()
            .unwrap();
        let child_id = child.id().clone();
        let parent: Arc<dyn IAgent> = Arc::new(MultiAgent::new("parent", vec![child]));
        let host = Arc::new(AgentHost::new(parent, store));

        let session = host.get_or_create_session("conv-2").await.unwrap();
        let stream = host
            .run(vec![ChatMessage::user("hello")], session, None)
            .await
            .unwrap();

        // Spawn a task that calls get_subagent while streaming
        let host_clone = host.clone();
        let child_id_clone = child_id.clone();
        let lookup_handle = tokio::spawn(async move {
            let mut found_count = 0;
            for _ in 0..10 {
                if host_clone.get_subagent(&child_id_clone).is_some() {
                    found_count += 1;
                }
                tokio::task::yield_now().await;
            }
            found_count
        });

        // Consume stream concurrently
        let results: Vec<_> = stream.collect().await;
        assert!(!results.is_empty());

        let lookup_count = lookup_handle.await.unwrap();
        assert_eq!(lookup_count, 10, "All get_subagent calls during streaming should succeed");
    }
}

use std::sync::Arc;

use rust_agent_core::{
    AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent, ISession,
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

    /// 获取内部 SessionStore 引用
    pub fn session_store(&self) -> &Arc<dyn ISessionStore> {
        &self.session_store
    }

    /// 获取 TTL 配置
    pub fn ttl_options(&self) -> Option<&SessionTTLOptions> {
        self.ttl_options.as_ref()
    }
}

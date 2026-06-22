//! RAF ↔ ACP session bridge.
//!
//! Manages the mapping between ACP `SessionId` and RAF `AgentSession`,
//! including per-session cancel tokens, target agent tracking, and
//! workflow runtime handles for HITL-capable agents.
//!
//! 当配置了 `session_store` 时，会话将持久化到文件系统，服务重启后可恢复。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rust_agent_core::{AgentSession, ISession, ISessionStore};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// 每个 ACP 会话存储的上下文。
#[derive(Clone)]
pub struct SessionContext {
    /// RAF Agent 会话。
    pub raf_session: Arc<AgentSession>,
    /// 此会话的目标 Agent ID。
    pub target_agent_id: String,
    /// 当前提示轮次的取消令牌。
    pub cancel_token: Option<Arc<AtomicBool>>,
    /// 是否为工作流会话（HITL-capable）。
    pub is_workflow: bool,
}

/// ACP 会话与 RAF Agent 会话之间的桥梁。
pub struct SessionBridge {
    sessions: RwLock<HashMap<String, SessionContext>>,
    /// 可选的会话持久化存储。设置后，会话在创建时从存储加载，在轮次结束后保存。
    session_store: Option<Arc<dyn ISessionStore>>,
}

impl SessionBridge {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            session_store: None,
        }
    }

    /// 创建带有会话持久化存储的 SessionBridge。
    pub fn with_store(session_store: Arc<dyn ISessionStore>) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            session_store: Some(session_store),
        }
    }

    /// 创建新会话，可选地指定目标 Agent。
    ///
    /// `is_workflow` 标记此会话是否使用工作流运行时（支持 HITL）。
    /// 如果配置了会话存储，会先尝试从存储中加载已有会话。
    pub async fn create_session(
        &self,
        session_id: &str,
        target_agent_id: Option<&str>,
        is_workflow: bool,
    ) -> anyhow::Result<()> {
        let raf_session = self.load_or_create_session(session_id).await;

        let ctx = SessionContext {
            raf_session,
            target_agent_id: target_agent_id.unwrap_or("default").to_string(),
            cancel_token: None,
            is_workflow,
        };
        self.sessions.write().await.insert(session_id.to_string(), ctx);
        Ok(())
    }

    /// 获取或为指定 ACP 会话 ID 创建 RAF 会话。
    ///
    /// 如果配置了会话存储，会先尝试从存储中加载。
    pub async fn get_or_create_raf_session(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Arc<AgentSession>> {
        let mut sessions = self.sessions.write().await;
        if let Some(ctx) = sessions.get(session_id) {
            return Ok(ctx.raf_session.clone());
        }

        let raf_session = self.load_or_create_session(session_id).await;

        let ctx = SessionContext {
            raf_session: raf_session.clone(),
            target_agent_id: "default".to_string(),
            cancel_token: None,
            is_workflow: false,
        };
        sessions.insert(session_id.to_string(), ctx);
        Ok(raf_session)
    }

    /// 从持久化存储加载会话，或创建新会话。
    ///
    /// 通过 serialize → deserialize 往返将 `dyn ISession` 转换为 `AgentSession`，
    /// 保留完整的消息历史和 Provider 状态。
    async fn load_or_create_session(&self, session_id: &str) -> Arc<AgentSession> {
        if let Some(ref store) = self.session_store {
            match store.get_session(session_id).await {
                Ok(Some(session)) => {
                    debug!(session_id, "Loaded session from store");
                    // serialize → deserialize 往返恢复 AgentSession
                    match session.serialize() {
                        Ok(json) => match AgentSession::deserialize(&json) {
                            Ok(s) => return Arc::new(s),
                            Err(e) => {
                                warn!(error = %e, "Failed to deserialize session, creating new");
                            }
                        },
                        Err(e) => {
                            warn!(error = %e, "Failed to serialize session, creating new");
                        }
                    }
                }
                Ok(None) => {
                    debug!(session_id, "No saved session found, creating new");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to load session from store, creating new");
                }
            }
        }
        Arc::new(AgentSession::with_id(session_id))
    }

    /// 获取会话上下文。
    pub async fn get_session_context(&self, session_id: &str) -> Option<SessionContext> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// 为会话注册取消令牌。
    pub async fn register_cancel_token(
        &self,
        session_id: &str,
        token: Arc<AtomicBool>,
    ) {
        if let Some(ctx) = self.sessions.write().await.get_mut(session_id) {
            ctx.cancel_token = Some(token);
        }
    }

    /// 触发会话的取消。
    pub async fn cancel_session(&self, session_id: &str) -> bool {
        if let Some(ctx) = self.sessions.read().await.get(session_id) {
            if let Some(ref token) = ctx.cancel_token {
                token.store(true, Ordering::SeqCst);
                return true;
            }
        }
        false
    }

    /// 获取会话的目标 Agent ID。
    pub async fn get_target_agent_id(&self, session_id: &str) -> Option<String> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|ctx| ctx.target_agent_id.clone())
    }

    /// 检查会话是否为工作流会话。
    pub async fn is_workflow_session(&self, session_id: &str) -> bool {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|ctx| ctx.is_workflow)
            .unwrap_or(false)
    }

    /// 将会话保存到持久化存储（如果配置了）。
    pub async fn save_session(&self, session_id: &str) -> anyhow::Result<()> {
        if let Some(ref store) = self.session_store {
            let sessions = self.sessions.read().await;
            if let Some(ctx) = sessions.get(session_id) {
                debug!(session_id, "Saving session to store");
                store.save_session(ctx.raf_session.as_ref()).await
                    .map_err(|e| anyhow::anyhow!("Failed to save session: {}", e))?;
            }
        }
        Ok(())
    }

    /// 移除会话。
    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }

    /// 移除会话的取消令牌（轮次完成后清理）。
    pub async fn clear_cancel_token(&self, session_id: &str) {
        if let Some(ctx) = self.sessions.write().await.get_mut(session_id) {
            ctx.cancel_token = None;
        }
    }
}

impl Default for SessionBridge {
    fn default() -> Self {
        Self::new()
    }
}

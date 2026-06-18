//! RAF ↔ ACP session bridge.
//!
//! Manages the mapping between ACP `SessionId` and RAF `AgentSession`,
//! including per-session cancel tokens and target agent tracking.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rust_agent_core::{AgentSession};
use tokio::sync::RwLock;

/// 每个 ACP 会话存储的上下文。
#[derive(Clone)]
pub struct SessionContext {
    /// RAF Agent 会话。
    pub raf_session: Arc<AgentSession>,
    /// 此会话的目标 Agent ID。
    pub target_agent_id: String,
    /// 当前提示轮次的取消令牌。
    pub cancel_token: Option<Arc<AtomicBool>>,
}

/// ACP 会话与 RAF Agent 会话之间的桥梁。
pub struct SessionBridge {
    sessions: RwLock<HashMap<String, SessionContext>>,
}

impl SessionBridge {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// 创建新会话，可选地指定目标 Agent。
    pub async fn create_session(
        &self,
        session_id: &str,
        target_agent_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let ctx = SessionContext {
            raf_session: Arc::new(AgentSession::with_id(session_id)),
            target_agent_id: target_agent_id.unwrap_or("default").to_string(),
            cancel_token: None,
        };
        self.sessions.write().await.insert(session_id.to_string(), ctx);
        Ok(())
    }

    /// 获取或为指定 ACP 会话 ID 创建 RAF 会话。
    pub async fn get_or_create_raf_session(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Arc<AgentSession>> {
        let mut sessions = self.sessions.write().await;
        let ctx = sessions.entry(session_id.to_string()).or_insert_with(|| {
            SessionContext {
                raf_session: Arc::new(AgentSession::with_id(session_id)),
                target_agent_id: "default".to_string(),
                cancel_token: None,
            }
        });
        Ok(ctx.raf_session.clone())
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

//! RAF ↔ ACP session bridge.
//!
//! Manages the mapping between ACP `SessionId` and RAF `AgentSession`,
//! including per-session cancel tokens and target agent tracking.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rust_agent_core::{AgentId, AgentSession, ChatMessage, ISession};
use tokio::sync::RwLock;

/// Context stored per ACP session.
#[derive(Clone)]
pub struct SessionContext {
    /// The RAF agent session.
    pub raf_session: Arc<AgentSession>,
    /// The target agent ID for this session.
    pub target_agent_id: String,
    /// Cancel token for the current prompt turn.
    pub cancel_token: Option<Arc<AtomicBool>>,
}

/// Bridge between ACP sessions and RAF agent sessions.
pub struct SessionBridge {
    sessions: RwLock<HashMap<String, SessionContext>>,
}

impl SessionBridge {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new session, optionally targeting a specific agent.
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

    /// Get or create a RAF session for the given ACP session ID.
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

    /// Get the session context.
    pub async fn get_session_context(&self, session_id: &str) -> Option<SessionContext> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Register a cancel token for a session.
    pub async fn register_cancel_token(
        &self,
        session_id: &str,
        token: Arc<AtomicBool>,
    ) {
        if let Some(ctx) = self.sessions.write().await.get_mut(session_id) {
            ctx.cancel_token = Some(token);
        }
    }

    /// Trigger cancellation for a session.
    pub async fn cancel_session(&self, session_id: &str) -> bool {
        if let Some(ctx) = self.sessions.read().await.get(session_id) {
            if let Some(ref token) = ctx.cancel_token {
                token.store(true, Ordering::SeqCst);
                return true;
            }
        }
        false
    }

    /// Get the target agent ID for a session.
    pub async fn get_target_agent_id(&self, session_id: &str) -> Option<String> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(|ctx| ctx.target_agent_id.clone())
    }

    /// Remove a session.
    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }

    /// Remove the cancel token for a session (cleanup after turn completes).
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

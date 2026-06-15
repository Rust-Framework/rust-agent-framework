use async_trait::async_trait;
use std::sync::Arc;

use crate::{ISession, Result};

/// Session persistence store interface.
///
/// Abstracts the storage backend for session data, enabling
/// cross-request and cross-restart session recovery.
///
/// Referenced from MAF's `AgentSessionStore` design.
#[async_trait]
pub trait ISessionStore: Send + Sync {
    /// Save a session to the store.
    ///
    /// If a session with the same ID already exists, it is overwritten.
    async fn save_session(&self, session: &dyn ISession) -> Result<()>;

    /// Get a session by ID.
    ///
    /// Returns `None` if no session with the given ID exists.
    async fn get_session(&self, session_id: &str) -> Result<Option<Arc<dyn ISession>>>;

    /// Delete a session by ID.
    ///
    /// No error is raised if the session does not exist.
    async fn delete_session(&self, session_id: &str) -> Result<()>;

    /// Clean up expired sessions.
    ///
    /// Returns the number of sessions removed.
    /// Implementations should check `ISession::last_active_at()` against
    /// the configured TTL options.
    async fn cleanup_expired(&self) -> Result<usize>;
}

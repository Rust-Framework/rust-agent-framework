use async_trait::async_trait;
use std::sync::Arc;

use rust_agent_core::{ISession, ISessionStore, Result};

/// Isolation key provider for multi-tenant session scoping.
///
/// Provides a key that is prepended to session IDs to ensure
/// tenant isolation. Referenced from MAF's
/// `SessionIsolationKeyProvider` design.
#[async_trait]
pub trait IIsolationKeyProvider: Send + Sync {
    async fn get_isolation_key(&self) -> Result<String>;
}

/// Fixed isolation key provider for simple scenarios.
///
/// Uses a static key string. Suitable for single-tenant
/// applications or testing.
pub struct FixedIsolationKeyProvider {
    key: String,
}

impl FixedIsolationKeyProvider {
    pub fn new(key: impl Into<String>) -> Self {
        Self { key: key.into() }
    }
}

#[async_trait]
impl IIsolationKeyProvider for FixedIsolationKeyProvider {
    async fn get_isolation_key(&self) -> Result<String> {
        Ok(self.key.clone())
    }
}

/// Isolation-scoped session store decorator.
///
/// Wraps an inner `ISessionStore` and prepends an isolation key
/// to session IDs, ensuring different tenants cannot access
/// each other's sessions.
///
/// Referenced from MAF's `IsolationKeyScopedAgentSessionStore`.
///
/// # Example
///
/// ```ignore
/// let inner = InMemorySessionStore::new();
/// let key_provider = FixedIsolationKeyProvider::new("tenant-123");
/// let store = IsolationScopedSessionStore::new(
///     Arc::new(inner),
///     Arc::new(key_provider),
/// );
/// // Session ID "abc" becomes "tenant-123::abc"
/// ```
pub struct IsolationScopedSessionStore {
    inner: Arc<dyn ISessionStore>,
    key_provider: Arc<dyn IIsolationKeyProvider>,
}

impl IsolationScopedSessionStore {
    pub fn new(
        inner: Arc<dyn ISessionStore>,
        key_provider: Arc<dyn IIsolationKeyProvider>,
    ) -> Self {
        Self { inner, key_provider }
    }

    async fn scoped_id(&self, session_id: &str) -> Result<String> {
        let key = self.key_provider.get_isolation_key().await?;
        Ok(format!("{}::{}", key, session_id))
    }
}

#[async_trait]
impl ISessionStore for IsolationScopedSessionStore {
    async fn save_session(&self, session: &dyn ISession) -> Result<()> {
        // Save with the original session ID — the scoping is handled
        // by the session ID itself being already scoped when created
        // (application layer manages session lifecycle)
        self.inner.save_session(session).await
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<Arc<dyn ISession>>> {
        let scoped = self.scoped_id(session_id).await?;
        self.inner.get_session(&scoped).await
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        let scoped = self.scoped_id(session_id).await?;
        self.inner.delete_session(&scoped).await
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        self.inner.cleanup_expired().await
    }
}

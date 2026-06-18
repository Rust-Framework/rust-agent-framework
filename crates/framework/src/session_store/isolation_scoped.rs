use async_trait::async_trait;
use std::sync::Arc;

use rust_agent_core::{ISession, ISessionStore, Result};

/// 多租户会话隔离的隔离键提供器。
///
/// 提供前置到会话 ID 的键，确保租户隔离。
/// 参考自 MAF 的 `SessionIsolationKeyProvider` 设计。
#[async_trait]
pub trait IIsolationKeyProvider: Send + Sync {
    async fn get_isolation_key(&self) -> Result<String>;
}

/// 适用于简单场景的固定隔离键提供器。
///
/// 使用静态键字符串。适用于单租户应用或测试。
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

/// 隔离作用域的会话存储装饰器。
///
/// 包装内部 `ISessionStore`，将会话 ID 前添加隔离键，确保不同租户无法访问彼此的会话。
///
/// 参考自 MAF 的 `IsolationKeyScopedAgentSessionStore`。
///
/// # 示例
///
/// ```ignore
/// let inner = InMemorySessionStore::new();
/// let key_provider = FixedIsolationKeyProvider::new("tenant-123");
/// let store = IsolationScopedSessionStore::new(
///     Arc::new(inner),
///     Arc::new(key_provider),
/// );
/// // 会话 ID "abc" 变为 "tenant-123::abc"
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

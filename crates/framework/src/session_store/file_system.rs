use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

use rust_agent_core::{AgentSession, ISession, ISessionStore, Result, AgentError};

/// File system session store.
///
/// Each session is stored as a JSON file in the configured directory.
/// File names are `{session_id}.json`.
///
/// Suitable for single-instance production deployments where
/// persistence across restarts is needed but a database is overkill.
pub struct FileSystemSessionStore {
    base_dir: PathBuf,
}

impl FileSystemSessionStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", session_id))
    }
}

#[async_trait]
impl ISessionStore for FileSystemSessionStore {
    async fn save_session(&self, session: &dyn ISession) -> Result<()> {
        // Ensure directory exists
        fs::create_dir_all(&self.base_dir).await.map_err(|e| {
            AgentError::Serialize(format!("Failed to create session directory: {}", e))
        })?;

        let json = session.serialize()?;
        let path = self.session_path(session.session_id());
        fs::write(&path, json).await.map_err(|e| {
            AgentError::Serialize(format!("Failed to write session file: {}", e))
        })?;
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<Arc<dyn ISession>>> {
        let path = self.session_path(session_id);
        match fs::read_to_string(&path).await {
            Ok(json) => {
                let session = AgentSession::deserialize(&json)?;
                Ok(Some(Arc::new(session)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AgentError::Serialize(format!(
                "Failed to read session file: {}", e
            )).into()),
        }
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        let path = self.session_path(session_id);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AgentError::Serialize(format!(
                "Failed to delete session file: {}", e
            )).into()),
        }
    }

    async fn cleanup_expired(&self) -> Result<usize> {
        // File-system store doesn't support TTL-based cleanup by default.
        // Could be extended to check file modification times.
        Ok(0)
    }
}

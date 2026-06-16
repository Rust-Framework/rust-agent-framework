use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;

use rust_agent_core::{AgentSession, ISession, ISessionStore, Result, AgentError, SessionTTLOptions};

/// File system session store.
///
/// Each session is stored as a JSON file in the configured directory.
/// File names are `{session_id}.json`.
///
/// Suitable for single-instance production deployments where
/// persistence across restarts is needed but a database is overkill.
///
/// ## TTL cleanup
///
/// When constructed with `with_ttl()`, `cleanup_expired()` will evict session
/// files whose `last_active_at` exceeds `max_idle_secs` or whose `created_at`
/// exceeds `max_lifetime_secs`.
pub struct FileSystemSessionStore {
    base_dir: PathBuf,
    ttl: Option<SessionTTLOptions>,
}

impl FileSystemSessionStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            ttl: None,
        }
    }

    /// Enable TTL-based session file cleanup.
    pub fn with_ttl(mut self, ttl: SessionTTLOptions) -> Self {
        self.ttl = Some(ttl);
        self
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.json", session_id))
    }
}

#[async_trait]
impl ISessionStore for FileSystemSessionStore {
    async fn save_session(&self, session: &dyn ISession) -> Result<()> {
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
            Err(e) => Err(AgentError::Serialize(format!("Failed to read session file: {}", e))),
        }
    }

    async fn delete_session(&self, session_id: &str) -> Result<()> {
        let path = self.session_path(session_id);
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(AgentError::Serialize(format!("Failed to delete session file: {}", e))),
        }
    }

    /// 清理过期 Session 文件，支持基于 mtime 的快速预过滤。
    ///
    /// # 算法流程
    ///
    /// 阶段 1 — mtime 预过滤（快速路径）：
    ///   读取文件的系统修改时间 `mtime`。
    ///   agent.run() 每次调用前后应用层可选择执行 save_session() 写入文件，
    ///   因此文件的 mtime 与最后一次 agent.run() 调用强相关。
    ///   如果 `now - mtime < max_idle_secs`，则该会话在空闲超时窗口内
    ///   肯定有活动记录，直接跳过 JSON 反序列化（O(1) 文件元数据 vs O(n) JSON 解析）。
    ///
    ///   注意：mtime 仅用于 idle 检查，不能用于 lifetime 检查。
    ///   一个刚被 touch 的会话可能已经超过了 max_lifetime_secs。
    ///
    /// 阶段 2 — JSON 时间戳验证（精确路径）：
    ///   仅对未通过 mtime 预过滤的文件进行完整 JSON 反序列化，
    ///   从 SessionSnapshot.last_active_at 和 SessionSnapshot.metadata.created_at
    ///   读取精确时间戳，与 TTL 配置比对。
    ///
    /// 阶段 3 — 批量删除：
    ///   收集所有过期/损坏的文件路径到 `to_delete` Vec 中，
    ///   循环调用 `std::fs::remove_file()` 逐个删除。
    ///   删除失败的文件通过 `tracing::warn!` 记录但不阻断流程。
    ///
    /// # 并发安全
    ///
    ///   单实例部署：文件级操作在此方法内串行执行，无并发风险。
    ///   多实例共享文件系统：存在 TOCTOU 竞态风险（见下方限制）。
    ///
    /// # 已知限制
    ///
    ///   - 不持有文件锁，多实例共享文件系统时存在 TOCTOU 窗口：
    ///     实例 A 判定文件过期 → 实例 B 刷新同一会话（覆盖写文件）
    ///     → 实例 A 删除文件 → 活跃会话被误删。
    ///     生产环境多实例部署建议使用数据库后端或引入分布式锁。
    ///   - `std::fs::read_dir` 在超大目录（10w+ 文件）存在性能瓶颈，
    ///     可考虑按时间分片存储或使用对象存储。
    ///   - cleanup 期间如果恰好有 agent.run() 正在写入该会话文件，
    ///     remove_file 和 write 操作可能交错（POSIX 允许），建议
    ///     在低流量窗口执行或使用 advisory lock。
    async fn cleanup_expired(&self) -> Result<usize> {
        let ttl = match &self.ttl {
            Some(t) => t,
            None => return Ok(0),
        };

        let now = chrono::Utc::now();
        let mut to_delete = Vec::new();

        let entries = match std::fs::read_dir(&self.base_dir) {
            Ok(d) => d.filter_map(|e| e.ok()).collect::<Vec<_>>(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => {
                return Err(AgentError::Serialize(format!(
                    "Failed to read session directory: {}", e
                )))
            }
        };

        for entry in entries {
            let path = entry.path();
            if path.extension().map_or(true, |ext| ext != "json") {
                continue;
            }

            // ── 阶段 1：mtime 预过滤 ──────────────────────────
            // 原理：agent.run() → session_store.save_session()
            // → fs::write(path, json) 更新文件 mtime。
            // 因此文件 mtime ≈ 最后一次 agent.run() 时间。
            //
            // 如果 now - mtime < max_idle_secs，则该会话在空闲超时
            // 窗口内必然有活动，可安全跳过昂贵的 JSON 反序列化。
            //
            // 局限：此优化仅适用于 idle 检查。lifetime 超时与 mtime
            // 无关（新创建的 session 也可能超出 lifetime），因此
            // 仍需在阶段 2 中进行完整的 lifetime 验证。
            if let Some(max_idle) = ttl.max_idle_secs {
                if let Ok(meta) = std::fs::metadata(&path) {
                    if let Ok(modified) = meta.modified() {
                        let modified_dt: chrono::DateTime<chrono::Utc> = modified.into();
                        let idle_duration = now - modified_dt;
                        if idle_duration.num_seconds() < max_idle as i64 {
                            tracing::trace!(
                                path = %path.display(),
                                mtime_secs = idle_duration.num_seconds(),
                                "mtime pre-filter: skipping active session"
                            );
                            continue;
                        }
                    }
                }
            }

            // ── 阶段 2：JSON 时间戳验证 ──────────────────────
            // 仅处理未通过 mtime 预过滤的文件，需要完整解析 JSON
            // 以获取精确的 last_active_at 和 created_at 时间戳。
            // 损坏/不可解析的文件也加入删除队列。
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => {
                    to_delete.push(path);
                    continue;
                }
            };

            let session = match AgentSession::deserialize(&content) {
                Ok(s) => s,
                Err(_) => {
                    to_delete.push(path);
                    continue;
                }
            };

            let last_active = session.last_active_at();
            let created = session.created_at();

            let mut expired = false;

            if let Some(max_idle) = ttl.max_idle_secs {
                let idle_duration = now - last_active;
                if idle_duration.num_seconds() > max_idle as i64 {
                    expired = true;
                }
            }

            if !expired {
                if let Some(max_lifetime) = ttl.max_lifetime_secs {
                    let lifetime_duration = now - created;
                    if lifetime_duration.num_seconds() > max_lifetime as i64 {
                        expired = true;
                    }
                }
            }

            if expired {
                to_delete.push(path);
            }
        }

        // ── 阶段 3：批量删除 ──────────────────────────────────
        // 先统计后删除：确保 removed 计数准确（即使部分删除失败）。
        // 逐个文件删除而非批量——POSIX 无批量删除 syscall，
        // 但路径收集避免了边遍历边删除的迭代器失效问题。
        // 删除失败时记录 warn 日志但不中断流程：残留文件
        // 将在下次 cleanup 时重试。
        let removed = to_delete.len();
        for path in to_delete {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Failed to remove expired session file"
                );
            }
        }

        if removed > 0 {
            tracing::info!(removed, "Expired session file eviction completed");
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::AgentSession;
    use tempfile::TempDir;

    fn temp_store_with_ttl(ttl: SessionTTLOptions) -> (TempDir, FileSystemSessionStore) {
        let dir = TempDir::new().expect("create temp dir");
        let store = FileSystemSessionStore::new(dir.path()).with_ttl(ttl);
        (dir, store)
    }

    #[tokio::test]
    async fn test_file_cleanup_expired_no_ttl() {
        let dir = TempDir::new().unwrap();
        let store = FileSystemSessionStore::new(dir.path());
        let s = Arc::new(AgentSession::with_id("s1"));
        store.save_session(s.as_ref()).await.unwrap();

        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 0, "No TTL should not remove any files");
    }

    #[tokio::test]
    async fn test_file_cleanup_expired_idle_timeout() {
        let ttl = SessionTTLOptions { max_idle_secs: Some(1), max_lifetime_secs: None, cleanup_interval_secs: 60 };
        let (_dir, store) = temp_store_with_ttl(ttl);

        let active = Arc::new(AgentSession::with_id("active"));
        let idle = Arc::new(AgentSession::with_id("idle"));
        store.save_session(active.as_ref()).await.unwrap();
        store.save_session(idle.as_ref()).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Touch active before cleanup
        let reloaded = store.get_session("active").await.unwrap().unwrap();
        reloaded.touch_last_active().await;
        store.save_session(reloaded.as_ref()).await.unwrap();

        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 1, "Should evict 1 idle session");
        assert!(store.get_session("active").await.unwrap().is_some());
        assert!(store.get_session("idle").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_file_cleanup_expired_lifetime_timeout() {
        let ttl = SessionTTLOptions { max_idle_secs: None, max_lifetime_secs: Some(1), cleanup_interval_secs: 60 };
        let (_dir, store) = temp_store_with_ttl(ttl);

        let s = Arc::new(AgentSession::with_id("old"));
        store.save_session(s.as_ref()).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 1);
    }

    #[tokio::test]
    async fn test_file_cleanup_expired_concurrent() {
        let ttl = SessionTTLOptions { max_idle_secs: Some(1), max_lifetime_secs: None, cleanup_interval_secs: 60 };
        let (_dir, store) = temp_store_with_ttl(ttl.clone());

        let mut handles = Vec::new();
        for i in 0..6 {
            let ttl = ttl.clone();
            handles.push(tokio::spawn({
                let store_path = _dir.path().to_path_buf();
                async move {
                    let store = FileSystemSessionStore::new(store_path).with_ttl(ttl);
                    let sid = format!("session-{}", i);
                    let s = Arc::new(AgentSession::with_id(&sid));
                    store.save_session(s.as_ref()).await.unwrap();
                    sid
                }
            }));
        }
        for h in handles { h.await.unwrap(); }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let removed = store.cleanup_expired().await.unwrap();
        assert_eq!(removed, 6, "All 6 sessions exceeded idle timeout");

        // Verify all removed
        for i in 0..6 {
            assert!(store.get_session(&format!("session-{}", i)).await.unwrap().is_none());
        }
    }
}

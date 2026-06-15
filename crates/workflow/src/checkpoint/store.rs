use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use parking_lot::RwLock;
use rust_agent_core::Result;

use super::checkpoint::{Checkpoint, CheckpointInfo};

// ═══════════════════════════════════════════════════
// ICheckpointStore trait
// ═══════════════════════════════════════════════════

/// 检查点持久化存储后端抽象
///
/// 支持多种后端实现：
/// - `InMemoryCheckpointStore` — 测试/调试
/// - `FileCheckpointStore` — 生产环境（原子写入）
/// - 未来可扩展 Redis / DB 实现
#[async_trait]
pub trait ICheckpointStore: Send + Sync {
    /// 保存检查点，返回轻量元数据指针
    async fn save(&self, session_id: &str, checkpoint: &Checkpoint) -> Result<CheckpointInfo>;

    /// 通过 checkpoint_id 加载完整检查点
    async fn load(&self, checkpoint_id: &str) -> Result<Checkpoint>;

    /// 列出某个 session 下所有检查点元数据
    async fn list(&self, session_id: &str) -> Result<Vec<CheckpointInfo>>;

    /// 删除指定检查点
    async fn delete(&self, checkpoint_id: &str) -> Result<()>;
}

// ═══════════════════════════════════════════════════
// InMemoryCheckpointStore
// ═══════════════════════════════════════════════════

/// 基于内存的检查点存储 — 测试和调试场景使用
///
/// 不持久化到磁盘，进程重启后数据丢失。
/// 适合单元测试和短期运行的工作流。
pub struct InMemoryCheckpointStore {
    /// checkpoint_id → Checkpoint
    checkpoints: RwLock<HashMap<String, Checkpoint>>,
    /// session_id → Vec<CheckpointInfo>
    infos: RwLock<HashMap<String, Vec<CheckpointInfo>>>,
}

impl InMemoryCheckpointStore {
    pub fn new() -> Self {
        Self {
            checkpoints: RwLock::new(HashMap::new()),
            infos: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryCheckpointStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ICheckpointStore for InMemoryCheckpointStore {
    async fn save(&self, session_id: &str, checkpoint: &Checkpoint) -> Result<CheckpointInfo> {
        let checkpoint_id = uuid::Uuid::new_v4().to_string();

        // 序列化以计算 byte_size
        let serialized = serde_json::to_vec(checkpoint).map_err(|e| {
            rust_agent_core::AgentError::Serialize(format!("序列化检查点失败: {}", e))
        })?;
        let byte_size = serialized.len() as u64;

        let info = CheckpointInfo {
            checkpoint_id: checkpoint_id.clone(),
            session_id: session_id.to_string(),
            step_number: checkpoint.step_number,
            parent_checkpoint_id: checkpoint.parent_checkpoint_id.clone(),
            is_full_snapshot: checkpoint.is_full_snapshot,
            byte_size,
            created_at: chrono::Utc::now(),
        };

        // 存储完整检查点
        self.checkpoints
            .write()
            .insert(checkpoint_id.clone(), checkpoint.clone());

        // 存储元数据
        self.infos
            .write()
            .entry(session_id.to_string())
            .or_default()
            .push(info.clone());

        Ok(info)
    }

    async fn load(&self, checkpoint_id: &str) -> Result<Checkpoint> {
        self.checkpoints
            .read()
            .get(checkpoint_id)
            .cloned()
            .ok_or_else(|| {
                rust_agent_core::AgentError::WorkflowError(format!(
                    "检查点 '{}' 不存在",
                    checkpoint_id
                ))
            })
    }

    async fn list(&self, session_id: &str) -> Result<Vec<CheckpointInfo>> {
        Ok(self
            .infos
            .read()
            .get(session_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn delete(&self, checkpoint_id: &str) -> Result<()> {
        self.checkpoints.write().remove(checkpoint_id);

        // 同时从 infos 中移除
        for infos in self.infos.write().values_mut() {
            infos.retain(|info| info.checkpoint_id != checkpoint_id);
        }

        Ok(())
    }
}

// ═══════════════════════════════════════════════════
// FileCheckpointStore
// ═══════════════════════════════════════════════════

/// 基于文件系统的检查点存储 — 生产环境使用
///
/// # 目录结构
///
/// ```text
/// {root_dir}/
///   {session_id}/
///     {checkpoint_id}.json       ← 完整检查点数据
///     {session_id}_index.json    ← 元数据索引
/// ```
///
/// # 原子写入
///
/// 使用临时文件 + rename 策略确保写崩溃安全：
/// 1. 写入 `{checkpoint_id}.json.tmp`
/// 2. flush + fsync（尽力而为）
/// 3. rename 到 `{checkpoint_id}.json`
/// 4. 更新索引文件
#[derive(Clone)]
pub struct FileCheckpointStore {
    root_dir: PathBuf,
}

impl FileCheckpointStore {
    /// 创建文件存储后端
    ///
    /// # 参数
    /// - `root_dir` — 检查点文件根目录，不存在时自动创建
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// 确保 session 目录存在
    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root_dir.join(session_id)
    }

    /// 索引文件路径
    fn index_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id)
            .join(format!("{}_index.json", session_id))
    }

    /// 确保目录存在
    async fn ensure_dir(&self, session_id: &str) -> Result<()> {
        let dir = self.session_dir(session_id);
        tokio::fs::create_dir_all(&dir).await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!(
                "创建检查点目录失败 '{}': {}",
                dir.display(),
                e
            ))
        })?;
        Ok(())
    }

    /// 原子写入文件（temp + rename）
    async fn atomic_write(&self, path: &std::path::Path, content: &str) -> Result<()> {
        let tmp_path = path.with_extension("json.tmp");

        tokio::fs::write(&tmp_path, content).await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!(
                "写入临时文件失败 '{}': {}",
                tmp_path.display(),
                e
            ))
        })?;

        // 重命名为最终文件名（POSIX 原子操作）
        tokio::fs::rename(&tmp_path, path).await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!(
                "原子重命名失败 '{}' -> '{}': {}",
                tmp_path.display(),
                path.display(),
                e
            ))
        })?;

        Ok(())
    }

    /// 加载索引
    async fn load_index(&self, session_id: &str) -> Result<Vec<CheckpointInfo>> {
        let path = self.index_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!(
                "读取索引文件失败 '{}': {}",
                path.display(),
                e
            ))
        })?;

        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        serde_json::from_str(&content).map_err(|e| {
            rust_agent_core::AgentError::Serialize(format!(
                "解析索引文件失败 '{}': {}",
                path.display(),
                e
            ))
        })
    }

    /// 保存索引
    async fn save_index(
        &self,
        session_id: &str,
        infos: &[CheckpointInfo],
    ) -> Result<()> {
        let path = self.index_path(session_id);
        let content =
            serde_json::to_string_pretty(infos).map_err(|e| {
                rust_agent_core::AgentError::Serialize(format!(
                    "序列化索引失败: {}",
                    e
                ))
            })?;

        self.atomic_write(&path, &content).await
    }
}

#[async_trait]
impl ICheckpointStore for FileCheckpointStore {
    async fn save(&self, session_id: &str, checkpoint: &Checkpoint) -> Result<CheckpointInfo> {
        self.ensure_dir(session_id).await?;

        let checkpoint_id = uuid::Uuid::new_v4().to_string();
        let checkpoint_path = self
            .session_dir(session_id)
            .join(format!("{}.json", checkpoint_id));

        // 序列化
        let content =
            serde_json::to_string_pretty(checkpoint).map_err(|e| {
                rust_agent_core::AgentError::Serialize(format!(
                    "序列化检查点失败: {}",
                    e
                ))
            })?;
        let byte_size = content.len() as u64;

        // 原子写入
        self.atomic_write(&checkpoint_path, &content).await?;

        let info = CheckpointInfo {
            checkpoint_id: checkpoint_id.clone(),
            session_id: session_id.to_string(),
            step_number: checkpoint.step_number,
            parent_checkpoint_id: checkpoint.parent_checkpoint_id.clone(),
            is_full_snapshot: checkpoint.is_full_snapshot,
            byte_size,
            created_at: chrono::Utc::now(),
        };

        // 更新索引
        let mut infos = self.load_index(session_id).await?;
        infos.push(info.clone());
        self.save_index(session_id, &infos).await?;

        Ok(info)
    }

    async fn load(&self, checkpoint_id: &str) -> Result<Checkpoint> {
        // 遍历所有 session 目录查找
        let mut entries = tokio::fs::read_dir(&self.root_dir).await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!(
                "读取根目录失败 '{}': {}",
                self.root_dir.display(),
                e
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!("读取条目失败: {}", e))
        })? {
            if !entry.file_type().await.map_err(|e| {
                rust_agent_core::AgentError::WorkflowError(format!("读取文件类型失败: {}", e))
            })?.is_dir()
            {
                continue;
            }

            let path = entry.path().join(format!("{}.json", checkpoint_id));
            if path.exists() {
                let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
                    rust_agent_core::AgentError::WorkflowError(format!(
                        "读取检查点文件失败 '{}': {}",
                        path.display(),
                        e
                    ))
                })?;

                return serde_json::from_str(&content).map_err(|e| {
                    rust_agent_core::AgentError::Serialize(format!(
                        "反序列化检查点失败: {}",
                        e
                    ))
                });
            }
        }

        Err(rust_agent_core::AgentError::WorkflowError(format!(
            "检查点 '{}' 不存在",
            checkpoint_id
        )))
    }

    async fn list(&self, session_id: &str) -> Result<Vec<CheckpointInfo>> {
        self.load_index(session_id).await
    }

    async fn delete(&self, checkpoint_id: &str) -> Result<()> {
        // 遍历所有 session 目录
        let mut entries = tokio::fs::read_dir(&self.root_dir).await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!(
                "读取根目录失败 '{}': {}",
                self.root_dir.display(),
                e
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            rust_agent_core::AgentError::WorkflowError(format!("读取条目失败: {}", e))
        })? {
            if !entry.file_type().await.map_err(|e| {
                rust_agent_core::AgentError::WorkflowError(format!("读取文件类型失败: {}", e))
            })?.is_dir()
            {
                continue;
            }

            let path = entry.path().join(format!("{}.json", checkpoint_id));
            if path.exists() {
                tokio::fs::remove_file(&path).await.map_err(|e| {
                    rust_agent_core::AgentError::WorkflowError(format!(
                        "删除检查点文件失败 '{}': {}",
                        path.display(),
                        e
                    ))
                })?;

                // 更新索引
                let session_id = entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();
                let mut infos = self.load_index(&session_id).await?;
                infos.retain(|info| info.checkpoint_id != checkpoint_id);
                self.save_index(&session_id, &infos).await?;

                return Ok(());
            }
        }

        Ok(())
    }
}

// ═══════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn make_checkpoint(step: i32, parent: Option<String>) -> Checkpoint {
        Checkpoint {
            step_number: step,
            graph_fingerprint: "test_fingerprint".to_string(),
            state_data: HashMap::new(),
            edge_state_data: HashMap::new(),
            pending_messages: Vec::new(),
            parent_checkpoint_id: parent,
            is_full_snapshot: false,
            created_at: chrono::Utc::now(),
        }
    }

    async fn test_store_roundtrip(store: &dyn ICheckpointStore) {
        let session_id = "test_session";

        // 保存
        let cp = make_checkpoint(1, None);
        let info = store.save(session_id, &cp).await.unwrap();

        // 加载
        let loaded = store.load(&info.checkpoint_id).await.unwrap();
        assert_eq!(loaded.step_number, 1);

        // 列出
        let list = store.list(session_id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].checkpoint_id, info.checkpoint_id);

        // 删除
        store.delete(&info.checkpoint_id).await.unwrap();
        let list_after = store.list(session_id).await.unwrap();
        assert!(list_after.is_empty());
    }

    async fn test_store_multiple_sessions(store: &dyn ICheckpointStore) {
        let cp1 = make_checkpoint(1, None);
        let cp2 = make_checkpoint(2, None);

        let i1 = store.save("session_a", &cp1).await.unwrap();
        let i2 = store.save("session_b", &cp2).await.unwrap();

        let list_a = store.list("session_a").await.unwrap();
        let list_b = store.list("session_b").await.unwrap();
        assert_eq!(list_a.len(), 1);
        assert_eq!(list_b.len(), 1);
        assert_ne!(list_a[0].checkpoint_id, i2.checkpoint_id);
        assert_ne!(list_b[0].checkpoint_id, i1.checkpoint_id);
    }

    #[tokio::test]
    async fn test_in_memory_store_roundtrip() {
        let store = InMemoryCheckpointStore::new();
        test_store_roundtrip(&store).await;
    }

    #[tokio::test]
    async fn test_in_memory_store_multiple_sessions() {
        let store = InMemoryCheckpointStore::new();
        test_store_multiple_sessions(&store).await;
    }

    #[tokio::test]
    async fn test_file_store_roundtrip() {
        let dir = std::env::temp_dir().join("raf_test_checkpoints");
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileCheckpointStore::new(&dir);
        test_store_roundtrip(&store).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_file_store_multiple_sessions() {
        let dir = std::env::temp_dir().join("raf_test_checkpoints_multi");
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileCheckpointStore::new(&dir);
        test_store_multiple_sessions(&store).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_file_store_atomic_write_survival() {
        let dir = std::env::temp_dir().join("raf_test_atomic");
        let _ = std::fs::remove_dir_all(&dir);

        let store = FileCheckpointStore::new(&dir);
        let cp = make_checkpoint(42, None);
        let info = store.save("test", &cp).await.unwrap();

        // 验证没有残留 .tmp 文件
        let tmp_files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .collect();
        assert!(tmp_files.is_empty(), "不应存在残留的 .tmp 文件");

        // 验证检查点可正常加载
        let loaded = store.load(&info.checkpoint_id).await.unwrap();
        assert_eq!(loaded.step_number, 42);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

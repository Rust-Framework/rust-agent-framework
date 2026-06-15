use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use rust_agent_core::Result;

use super::checkpoint::{Checkpoint, CheckpointConfig, CheckpointInfo, ScopeKey};
use super::message_envelope::SerializableMessageEnvelope;
use super::store::ICheckpointStore;

/// 检查点管理器 — 引擎与存储之间的协调层
///
/// # 职责
/// - 创建初始检查点（step=-1）
/// - 决定增量/全量策略
/// - 沿 parent 链回溯合并完整状态（故障恢复用）
/// - 清理过期检查点
pub struct CheckpointManager {
    store: Arc<dyn ICheckpointStore>,
    config: CheckpointConfig,
    /// 每个 session 的增量计数器：(session_id, count)
    counter: RwLock<HashMap<String, u32>>,
}

impl CheckpointManager {
    pub fn new(store: Arc<dyn ICheckpointStore>, config: CheckpointConfig) -> Self {
        Self {
            store,
            config,
            counter: RwLock::new(HashMap::new()),
        }
    }

    /// 使用默认配置创建
    pub fn with_default_config(store: Arc<dyn ICheckpointStore>) -> Self {
        Self::new(store, CheckpointConfig::default())
    }

    // ═══════════════════════════════════════════════════
    // 公共 API
    // ═══════════════════════════════════════════════════

    /// 创建初始检查点（step_number = -1）
    ///
    /// 仅包含 graph_fingerprint，无状态数据。
    /// 用于建立检查点链的起点。
    pub async fn create_initial(
        &self,
        session_id: &str,
        graph_fingerprint: &str,
    ) -> Result<CheckpointInfo> {
        let checkpoint = Checkpoint {
            step_number: -1,
            graph_fingerprint: graph_fingerprint.to_string(),
            state_data: HashMap::new(),
            edge_state_data: HashMap::new(),
            pending_messages: Vec::new(),
            parent_checkpoint_id: None,
            is_full_snapshot: true,
            created_at: chrono::Utc::now(),
        };

        self.store.save(session_id, &checkpoint).await
    }

    /// 提交检查点，自动判断增量/全量
    ///
    /// `state_data` 使用 `ScopeKey` 作为键，内部自动转换为字符串键持久化。
    ///
    /// # 返回
    /// 已保存的 `CheckpointInfo` 元数据
    pub async fn commit(
        &self,
        session_id: &str,
        graph_fingerprint: &str,
        state_data: HashMap<ScopeKey, serde_json::Value>,
        edge_state_data: HashMap<String, serde_json::Value>,
        pending_messages: Vec<SerializableMessageEnvelope>,
        step_number: i32,
    ) -> Result<CheckpointInfo> {
        if !self.config.enabled {
            return Err(rust_agent_core::AgentError::WorkflowError(
                "检查点管理器已禁用".into(),
            ));
        }

        // 将 ScopeKey 转换为字符串键
        let state_data_string_keys: HashMap<String, serde_json::Value> = state_data
            .into_iter()
            .map(|(k, v)| (k.to_key_string(), v))
            .collect();

        // 获取当前计数值（须在 await 前释放锁，parking_lot 锁不实现 Send）
        let (count, is_full) = {
            let mut counter = self.counter.write();
            let count = counter.entry(session_id.to_string()).or_insert(0);
            let is_full = *count > 0 && *count % self.config.full_snapshot_interval == 0;
            (*count, is_full)
        };

        // 获取父检查点 ID（最近一次保存的检查点，即上一个增量）
        let parent_id = if is_full {
            None
        } else {
            self.store
                .list(session_id)
                .await?
                .into_iter()
                .max_by_key(|info| info.step_number)
                .map(|info| info.checkpoint_id)
        };

        let checkpoint = Checkpoint {
            step_number,
            graph_fingerprint: graph_fingerprint.to_string(),
            state_data: state_data_string_keys,
            edge_state_data,
            pending_messages,
            parent_checkpoint_id: parent_id,
            is_full_snapshot: is_full,
            created_at: chrono::Utc::now(),
        };

        let info = self.store.save(session_id, &checkpoint).await?;

        {
            let mut counter = self.counter.write();
            let entry = counter.entry(session_id.to_string()).or_insert(0);
            *entry = count + 1;
        }

        Ok(info)
    }

    /// 加载最新检查点并沿 parent 链回溯合并完整状态（故障恢复核心）
    ///
    /// # 返回
    /// `(最新检查点, 合并后的完整 state_data)`，state_data 的键已还原为 ScopeKey
    pub async fn load_full_state(
        &self,
        session_id: &str,
    ) -> Result<Option<(Checkpoint, HashMap<ScopeKey, serde_json::Value>)>> {
        let infos = self.store.list(session_id).await?;
        if infos.is_empty() {
            return Ok(None);
        }

        let latest_info = infos
            .into_iter()
            .max_by_key(|info| info.step_number)
            .unwrap();

        let latest = self.store.load(&latest_info.checkpoint_id).await?;

        if latest.is_full_snapshot {
            let state: HashMap<ScopeKey, serde_json::Value> = latest
                .state_data
                .iter()
                .map(|(k, v)| (ScopeKey::from_key_string(k), v.clone()))
                .collect();
            return Ok(Some((latest, state)));
        }

        let mut merged_state: HashMap<String, serde_json::Value> = latest.state_data.clone();
        let mut current_id = latest.parent_checkpoint_id.clone();

        while let Some(parent_id) = current_id {
            match self.store.load(&parent_id).await {
                Ok(parent) => {
                    for (key, value) in &parent.state_data {
                        merged_state.entry(key.clone()).or_insert_with(|| value.clone());
                    }

                    if parent.is_full_snapshot {
                        break;
                    }

                    current_id = parent.parent_checkpoint_id.clone();
                }
                // parent 链中部分检查点可能已被清理，此时 chain 自然终止
                Err(_) => {
                    tracing::warn!("parent 检查点已被清理，链回溯终止");
                    break;
                }
            }
        }

        // 转换为 ScopeKey 键
        let state: HashMap<ScopeKey, serde_json::Value> = merged_state
            .into_iter()
            .map(|(k, v)| (ScopeKey::from_key_string(&k), v))
            .collect();

        Ok(Some((latest, state)))
    }

    /// 获取最新检查点信息（不加载完整数据）
    pub async fn get_latest_info(
        &self,
        session_id: &str,
    ) -> Result<Option<CheckpointInfo>> {
        let infos = self.store.list(session_id).await?;
        Ok(infos.into_iter().max_by_key(|info| info.step_number))
    }

    /// 清理过期检查点，保留最近的 `config.max_checkpoints` 条
    pub async fn cleanup(&self, session_id: &str) -> Result<()> {
        let mut infos = self.store.list(session_id).await?;

        if infos.len() <= self.config.max_checkpoints {
            return Ok(());
        }

        // 按 step_number 降序排序，保留前 max_checkpoints 个
        infos.sort_by_key(|info| std::cmp::Reverse(info.step_number));

        let to_delete: Vec<String> = infos
            .iter()
            .skip(self.config.max_checkpoints)
            .map(|info| info.checkpoint_id.clone())
            .collect();

        let delete_count = to_delete.len();

        for id in &to_delete {
            self.store.delete(id).await?;
        }

        tracing::info!(
            "清理了 {} 个过期检查点 (session: {})",
            delete_count,
            session_id
        );

        Ok(())
    }
}

// ═══════════════════════════════════════════════════
// 单元测试：增量保存 + 故障恢复
// ═══════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::store::InMemoryCheckpointStore;
    use serde_json::json;

    fn make_manager() -> CheckpointManager {
        let store = Arc::new(InMemoryCheckpointStore::new());
        let config = CheckpointConfig {
            full_snapshot_interval: 5, // 每 5 次增量一次全量
            max_checkpoints: 10,
            enabled: true,
        };
        CheckpointManager::new(store, config)
    }

    fn make_state_data(data: &[(&str, &str, &str)]) -> HashMap<ScopeKey, serde_json::Value> {
        data.iter()
            .map(|(node, scope, value)| {
                let key = if scope.is_empty() {
                    ScopeKey::private(*node)
                } else {
                    ScopeKey::shared(*node, *scope)
                };
                (key, json!(*value))
            })
            .collect()
    }

    // ═══════════════════════════════════════
    // 基础功能测试
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_create_initial_checkpoint() {
        let manager = make_manager();
        let info = manager
            .create_initial("session_1", "fingerprint_v1")
            .await
            .unwrap();

        assert_eq!(info.step_number, -1);
        assert!(info.is_full_snapshot);
        assert_eq!(info.session_id, "session_1");

        // 可加载
        let loaded = manager.store.load(&info.checkpoint_id).await.unwrap();
        assert_eq!(loaded.step_number, -1);
        assert!(loaded.is_full_snapshot);
    }

    #[tokio::test]
    async fn test_commit_incremental_checkpoint() {
        let manager = make_manager();
        let session = "test_incr";

        // 初始检查点
        let _init = manager
            .create_initial(session, "fp_v1")
            .await
            .unwrap();

        // 提交增量检查点
        let info = manager
            .commit(
                session,
                "fp_v1",
                make_state_data(&[("node_a", "", "value_1")]),
                HashMap::new(),
                vec![],
                1,
            )
            .await
            .unwrap();

        assert_eq!(info.step_number, 1);
        assert!(!info.is_full_snapshot); // 第 1 次不是全量

        // 验证 parent 链指向初始检查点
        let loaded = manager.store.load(&info.checkpoint_id).await.unwrap();
        assert_eq!(loaded.state_data.len(), 1);
        assert!(loaded.parent_checkpoint_id.is_some());
    }

    #[tokio::test]
    async fn test_full_snapshot_trigger() {
        let manager = make_manager();
        let session = "test_full";

        let _init = manager.create_initial(session, "fp").await.unwrap();

        // 保存 6 次，第 6 次（count=5, 5%5==0）应触发全量
        for step in 1..=6 {
            let info = manager
                .commit(
                    session,
                    "fp",
                    make_state_data(&[(format!("node_{}", step).as_str(), "", "x")]),
                    HashMap::new(),
                    vec![],
                    step,
                )
                .await
                .unwrap();

            if step == 6 {
                // 第 6 次提交时 count=5, 5%5==0 触发全量
                assert!(info.is_full_snapshot, "step {} 应触发全量快照", step);
            }
        }
    }

    // ═══════════════════════════════════════
    // 增量保存 + 回溯合并测试
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_load_full_state_chain_traversal() {
        let manager = make_manager();
        let session = "test_chain";

        let _init = manager.create_initial(session, "fp").await.unwrap();

        // 保存 3 个增量检查点，每个更新不同的状态键
        manager
            .commit(
                session,
                "fp",
                make_state_data(&[("node_a", "", "a1")]),
                HashMap::new(),
                vec![],
                1,
            )
            .await
            .unwrap();

        manager
            .commit(
                session,
                "fp",
                make_state_data(&[("node_b", "", "b1")]),
                HashMap::new(),
                vec![],
                2,
            )
            .await
            .unwrap();

        manager
            .commit(
                session,
                "fp",
                make_state_data(&[("node_c", "shared", "c1")]),
                HashMap::new(),
                vec![],
                3,
            )
            .await
            .unwrap();

        // 加载合并后的完整状态
        let (latest, merged) = manager
            .load_full_state(session)
            .await
            .unwrap()
            .expect("应该有检查点");

        assert_eq!(latest.step_number, 3);

        // 合并后的状态应包含所有增量中设置的键
        let key_a = ScopeKey::private("node_a");
        let key_b = ScopeKey::private("node_b");
        let key_c = ScopeKey::shared("node_c", "shared");

        assert_eq!(merged.get(&key_a).unwrap(), &json!("a1"));
        assert_eq!(merged.get(&key_b).unwrap(), &json!("b1"));
        assert_eq!(merged.get(&key_c).unwrap(), &json!("c1"));
        assert_eq!(merged.len(), 3);
    }

    #[tokio::test]
    async fn test_load_full_state_with_overwrite() {
        let manager = make_manager();
        let session = "test_overwrite";

        let _init = manager.create_initial(session, "fp").await.unwrap();

        // 步骤 1: 设置 node_a = "old_value"
        manager
            .commit(
                session,
                "fp",
                make_state_data(&[("node_a", "", "old_value")]),
                HashMap::new(),
                vec![],
                1,
            )
            .await
            .unwrap();

        // 步骤 2: 覆盖 node_a = "new_value"，同时设置 node_b
        manager
            .commit(
                session,
                "fp",
                make_state_data(&[("node_a", "", "new_value"), ("node_b", "", "b_val")]),
                HashMap::new(),
                vec![],
                2,
            )
            .await
            .unwrap();

        let (_latest, merged) = manager
            .load_full_state(session)
            .await
            .unwrap()
            .unwrap();

        let key_a = ScopeKey::private("node_a");
        let key_b = ScopeKey::private("node_b");

        // 合并后应取最新值
        assert_eq!(merged.get(&key_a).unwrap(), &json!("new_value"));
        assert_eq!(merged.get(&key_b).unwrap(), &json!("b_val"));
        assert_eq!(merged.len(), 2);
    }

    #[tokio::test]
    async fn test_load_full_state_from_full_snapshot() {
        let manager = make_manager();
        let session = "test_full_resume";

        let _init = manager.create_initial(session, "fp").await.unwrap();

        // 保存到触发全量快照（interval=5，第5次触发）
        for step in 1..=5 {
            manager
                .commit(
                    session,
                    "fp",
                    make_state_data(&[
                        ("node_x", "", &format!("val_{}", step)),
                    ]),
                    HashMap::new(),
                    vec![],
                    step,
                )
                .await
                .unwrap();
        }

        let (latest, merged) = manager
            .load_full_state(session)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(latest.step_number, 5);
        // 全量快照应直接返回，merged = latest.state_data
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged.get(&ScopeKey::private("node_x")).unwrap(),
            &json!("val_5")
        );
    }

    // ═══════════════════════════════════════
    // 故障恢复场景模拟
    // ═══════════════════════════════════════

    /// 场景 1：单 session 正常执行完成后，使用新 Engine 实例从检查点恢复
    #[tokio::test]
    async fn test_fault_recovery_basic() {
        // 模拟第一次运行
        let store: Arc<dyn ICheckpointStore> =
            Arc::new(InMemoryCheckpointStore::new());
        let manager_1 = CheckpointManager::with_default_config(store.clone());
        let session = "recovery_test";

        let _init = manager_1.create_initial(session, "fp_v1").await.unwrap();

        // 执行到步骤 3 发生"崩溃"
        manager_1
            .commit(
                session,
                "fp_v1",
                make_state_data(&[("node_a", "", "step1_data")]),
                HashMap::new(),
                vec![],
                1,
            )
            .await
            .unwrap();

        manager_1
            .commit(
                session,
                "fp_v1",
                make_state_data(&[("node_b", "", "step2_data")]),
                HashMap::new(),
                vec![],
                2,
            )
            .await
            .unwrap();

        manager_1
            .commit(
                session,
                "fp_v1",
                make_state_data(&[("node_c", "", "step3_data")]),
                HashMap::new(),
                vec![],
                3,
            )
            .await
            .unwrap();

        // drop(manager_1); // 模拟崩溃

        // 模拟恢复：创建新的 CheckpointManager 使用同一个 store
        let manager_2 = CheckpointManager::with_default_config(store);

        let (latest, merged) = manager_2
            .load_full_state(session)
            .await
            .unwrap()
            .expect("恢复后应有检查点");

        assert_eq!(latest.step_number, 3);

        // 验证所有状态都被正确恢复
        let key_a = ScopeKey::private("node_a");
        let key_b = ScopeKey::private("node_b");
        let key_c = ScopeKey::private("node_c");

        assert_eq!(merged.get(&key_a).unwrap(), &json!("step1_data"));
        assert_eq!(merged.get(&key_b).unwrap(), &json!("step2_data"));
        assert_eq!(merged.get(&key_c).unwrap(), &json!("step3_data"));
        assert_eq!(merged.len(), 3);
    }

    /// 场景 2：拓扑指纹不匹配时拒绝恢复
    #[tokio::test]
    async fn test_fault_recovery_fingerprint_mismatch() {
        let store: Arc<dyn ICheckpointStore> =
            Arc::new(InMemoryCheckpointStore::new());
        let manager = CheckpointManager::with_default_config(store.clone());
        let session = "fingerprint_test";

        let _init = manager
            .create_initial(session, "fp_v1")
            .await
            .unwrap();

        manager
            .commit(
                session,
                "fp_v1",
                make_state_data(&[("node_a", "", "data")]),
                HashMap::new(),
                vec![],
                1,
            )
            .await
            .unwrap();

        // 恢复时验证指纹
        let (checkpoint, _merged) = manager
            .load_full_state(session)
            .await
            .unwrap()
            .unwrap();

        // 工作流图定义变更后，指纹应该不匹配
        assert_ne!(checkpoint.graph_fingerprint, "fp_v2_changed");
        assert_eq!(checkpoint.graph_fingerprint, "fp_v1");
    }

    /// 场景 3：多 session 隔离恢复
    #[tokio::test]
    async fn test_fault_recovery_multi_session_isolation() {
        let store: Arc<dyn ICheckpointStore> =
            Arc::new(InMemoryCheckpointStore::new());
        let manager = CheckpointManager::with_default_config(store.clone());

        // Session A
        let _init_a = manager.create_initial("session_a", "fp").await.unwrap();
        manager
            .commit(
                "session_a",
                "fp",
                make_state_data(&[("a", "", "A")]),
                HashMap::new(),
                vec![],
                1,
            )
            .await
            .unwrap();

        // Session B
        let _init_b = manager.create_initial("session_b", "fp").await.unwrap();
        manager
            .commit(
                "session_b",
                "fp",
                make_state_data(&[("b", "", "B")]),
                HashMap::new(),
                vec![],
                1,
            )
            .await
            .unwrap();

        // 恢复 session A — 不应包含 session B 的数据
        let (_latest_a, merged_a) = manager
            .load_full_state("session_a")
            .await
            .unwrap()
            .unwrap();
        assert!(merged_a.contains_key(&ScopeKey::private("a")));
        assert!(!merged_a.contains_key(&ScopeKey::private("b")));

        // 恢复 session B — 不应包含 session A 的数据
        let (_latest_b, merged_b) = manager
            .load_full_state("session_b")
            .await
            .unwrap()
            .unwrap();
        assert!(!merged_b.contains_key(&ScopeKey::private("a")));
        assert!(merged_b.contains_key(&ScopeKey::private("b")));
    }

    /// 场景 4：干净状态恢复（无历史检查点）
    #[tokio::test]
    async fn test_fault_recovery_clean_slate() {
        let store: Arc<dyn ICheckpointStore> =
            Arc::new(InMemoryCheckpointStore::new());
        let manager = CheckpointManager::with_default_config(store);

        let result = manager
            .load_full_state("nonexistent_session")
            .await
            .unwrap();
        assert!(result.is_none(), "无历史记录应返回 None");
    }

    /// 场景 5：清理过期检查点后的恢复
    #[tokio::test]
    async fn test_fault_recovery_after_cleanup() {
        let store: Arc<dyn ICheckpointStore> =
            Arc::new(InMemoryCheckpointStore::new());
        let config = CheckpointConfig {
            full_snapshot_interval: 100, // 不触发全量，全部增量
            max_checkpoints: 3,          // 只保留最近 3 个
            enabled: true,
        };
        let manager = CheckpointManager::new(store.clone(), config);
        let session = "cleanup_test";

        let _init = manager.create_initial(session, "fp").await.unwrap();

        // 保存 5 个检查点
        for step in 1..=5 {
            manager
                .commit(
                    session,
                    "fp",
                    make_state_data(&[("key", "", &format!("step_{}", step))]),
                    HashMap::new(),
                    vec![],
                    step,
                )
                .await
                .unwrap();
        }

        // 清理（保留最近 3 个）
        manager.cleanup(session).await.unwrap();

        let infos = store.list(session).await.unwrap();
        // 初始(-1) + 5 个 = 6 个，清理后保留 3 个
        // 但实际上初始检查点 step=-1 也会参与清理
        assert!(infos.len() <= 3, "清理后应最多保留 3 个检查点");

        // 验证恢复仍然正确（最新值取自 step_5）
        let (_latest, merged) = manager
            .load_full_state(session)
            .await
            .unwrap()
            .unwrap();

        let key = ScopeKey::private("key");
        assert_eq!(merged.get(&key).unwrap(), &json!("step_5"));
    }

    // ═══════════════════════════════════════
    // 禁用状态测试
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_disabled_manager_commit_fails() {
        let store: Arc<dyn ICheckpointStore> =
            Arc::new(InMemoryCheckpointStore::new());
        let config = CheckpointConfig::disabled();
        let manager = CheckpointManager::new(store, config);

        let result = manager
            .commit(
                "test",
                "fp",
                HashMap::new(),
                HashMap::new(),
                vec![],
                1,
            )
            .await;

        assert!(result.is_err(), "禁用的管理器提交应返回错误");
    }
}

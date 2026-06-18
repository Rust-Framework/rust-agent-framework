//! Sub-agent status tracker.
//!
//! Tracks the execution status of sub-agents during orchestration,
//! sending status-change signals via `session/update` notifications.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use serde::Serialize;

/// 子 Agent 的执行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubAgentStatus {
    Pending,
    Executing,
    Completed,
    Error,
}

/// 被追踪的子 Agent 的状态。
#[derive(Debug, Clone)]
pub struct SubAgentState {
    pub agent_type: String,
    pub status: SubAgentStatus,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
}

/// 追踪多 Agent 编排中子 Agent 的执行状态。
pub struct SubAgentStatusTracker {
    agents: Mutex<HashMap<String, SubAgentState>>,
}

impl SubAgentStatusTracker {
    pub fn new() -> Self {
        Self {
            agents: Mutex::new(HashMap::new()),
        }
    }

    /// 注册一个子 Agent 进行追踪。
    pub fn register(&self, id: &str, agent_type: &str) {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        agents.entry(id.to_string()).or_insert_with(|| SubAgentState {
            agent_type: agent_type.to_string(),
            status: SubAgentStatus::Pending,
            started_at: None,
            completed_at: None,
        });
    }

    /// 一次注册多个子 Agent。
    pub fn register_all(&self, ids: &[(String, String)]) {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        for (id, agent_type) in ids {
            agents.entry(id.clone()).or_insert_with(|| SubAgentState {
                agent_type: agent_type.clone(),
                status: SubAgentStatus::Pending,
                started_at: None,
                completed_at: None,
            });
        }
    }

    /// 将子 Agent 标记为执行中（如果尚未）。
    /// 如果状态发生更改则返回 `true`。
    pub fn ensure_active(&self, id: &str) -> bool {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = agents.get_mut(id) {
            if state.status == SubAgentStatus::Pending {
                state.status = SubAgentStatus::Executing;
                state.started_at = Some(Instant::now());
                return true;
            }
        }
        false
    }

    /// 将子 Agent 标记为已完成。
    /// 如果状态发生更改则返回 `true`。
    pub fn mark_completed(&self, id: &str) -> bool {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = agents.get_mut(id) {
            if state.status != SubAgentStatus::Completed {
                state.status = SubAgentStatus::Completed;
                state.completed_at = Some(Instant::now());
                return true;
            }
        }
        false
    }

    /// 将子 Agent 标记为错误。
    /// 如果状态发生更改则返回 `true`。
    pub fn mark_error(&self, id: &str) -> bool {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = agents.get_mut(id) {
            if state.status != SubAgentStatus::Error {
                state.status = SubAgentStatus::Error;
                state.completed_at = Some(Instant::now());
                return true;
            }
        }
        false
    }

    /// 将所有追踪的 Agent 标记为已完成。
    pub fn mark_all_completed(&self) {
        let mut agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        for state in agents.values_mut() {
            if state.status == SubAgentStatus::Executing || state.status == SubAgentStatus::Pending {
                state.status = SubAgentStatus::Completed;
                state.completed_at = Some(Instant::now());
            }
        }
    }

    /// 构建状态元数据，用于包含在 `session/update._meta` 中。
    pub fn build_status_meta(&self) -> serde_json::Value {
        let agents = self.agents.lock().unwrap_or_else(|e| e.into_inner());
        let statuses: Vec<serde_json::Value> = agents
            .iter()
            .map(|(id, state)| {
                serde_json::json!({
                    "id": id,
                    "type": state.agent_type,
                    "status": state.status,
                    "elapsed_ms": state.started_at.map(|t| t.elapsed().as_millis() as u64),
                })
            })
            .collect();
        serde_json::json!({ "sub_agents": statuses })
    }

    /// 获取特定 Agent 的状态。
    pub fn get_status(&self, id: &str) -> Option<SubAgentStatus> {
        self.agents.lock().unwrap_or_else(|e| e.into_inner()).get(id).map(|s| s.status)
    }

    /// 检查所有追踪的 Agent 是否均处于终止状态。
    pub fn all_terminated(&self) -> bool {
        self.agents
            .lock()
            .unwrap()
            .values()
            .all(|s| matches!(s.status, SubAgentStatus::Completed | SubAgentStatus::Error))
    }
}

impl Default for SubAgentStatusTracker {
    fn default() -> Self {
        Self::new()
    }
}

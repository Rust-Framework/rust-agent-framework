//! Sub-agent status tracker.
//!
//! Tracks the execution status of sub-agents during orchestration,
//! sending status-change signals via `session/update` notifications.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use serde::Serialize;

/// Execution status of a sub-agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubAgentStatus {
    Pending,
    Executing,
    Completed,
    Error,
}

/// State of a tracked sub-agent.
#[derive(Debug, Clone)]
pub struct SubAgentState {
    pub agent_type: String,
    pub status: SubAgentStatus,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
}

/// Tracks sub-agent execution status for multi-agent orchestration.
pub struct SubAgentStatusTracker {
    agents: Mutex<HashMap<String, SubAgentState>>,
}

impl SubAgentStatusTracker {
    pub fn new() -> Self {
        Self {
            agents: Mutex::new(HashMap::new()),
        }
    }

    /// Register a sub-agent for tracking.
    pub fn register(&self, id: &str, agent_type: &str) {
        let mut agents = self.agents.lock().unwrap();
        agents.entry(id.to_string()).or_insert_with(|| SubAgentState {
            agent_type: agent_type.to_string(),
            status: SubAgentStatus::Pending,
            started_at: None,
            completed_at: None,
        });
    }

    /// Register multiple sub-agents at once.
    pub fn register_all(&self, ids: &[(String, String)]) {
        let mut agents = self.agents.lock().unwrap();
        for (id, agent_type) in ids {
            agents.entry(id.clone()).or_insert_with(|| SubAgentState {
                agent_type: agent_type.clone(),
                status: SubAgentStatus::Pending,
                started_at: None,
                completed_at: None,
            });
        }
    }

    /// Mark a sub-agent as executing (if not already).
    /// Returns `true` if the status changed.
    pub fn ensure_active(&self, id: &str) -> bool {
        let mut agents = self.agents.lock().unwrap();
        if let Some(state) = agents.get_mut(id) {
            if state.status == SubAgentStatus::Pending {
                state.status = SubAgentStatus::Executing;
                state.started_at = Some(Instant::now());
                return true;
            }
        }
        false
    }

    /// Mark a sub-agent as completed.
    /// Returns `true` if the status changed.
    pub fn mark_completed(&self, id: &str) -> bool {
        let mut agents = self.agents.lock().unwrap();
        if let Some(state) = agents.get_mut(id) {
            if state.status != SubAgentStatus::Completed {
                state.status = SubAgentStatus::Completed;
                state.completed_at = Some(Instant::now());
                return true;
            }
        }
        false
    }

    /// Mark a sub-agent as error.
    /// Returns `true` if the status changed.
    pub fn mark_error(&self, id: &str) -> bool {
        let mut agents = self.agents.lock().unwrap();
        if let Some(state) = agents.get_mut(id) {
            if state.status != SubAgentStatus::Error {
                state.status = SubAgentStatus::Error;
                state.completed_at = Some(Instant::now());
                return true;
            }
        }
        false
    }

    /// Mark all tracked agents as completed.
    pub fn mark_all_completed(&self) {
        let mut agents = self.agents.lock().unwrap();
        for state in agents.values_mut() {
            if state.status == SubAgentStatus::Executing || state.status == SubAgentStatus::Pending {
                state.status = SubAgentStatus::Completed;
                state.completed_at = Some(Instant::now());
            }
        }
    }

    /// Build status meta for inclusion in `session/update._meta`.
    pub fn build_status_meta(&self) -> serde_json::Value {
        let agents = self.agents.lock().unwrap();
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

    /// Get the status of a specific agent.
    pub fn get_status(&self, id: &str) -> Option<SubAgentStatus> {
        self.agents.lock().unwrap().get(id).map(|s| s.status)
    }

    /// Check if all tracked agents are in a terminal state.
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

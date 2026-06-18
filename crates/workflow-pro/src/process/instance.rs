use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use super::definition::ProcessDefinition;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessState { Created, Running, Suspended, Completed, Terminated, Failed }
impl std::fmt::Display for ProcessState { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { match self { Self::Created => write!(f, "created"), Self::Running => write!(f, "running"), Self::Suspended => write!(f, "suspended"), Self::Completed => write!(f, "completed"), Self::Terminated => write!(f, "terminated"), Self::Failed => write!(f, "failed") } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSnapshot { pub process_id: String, pub definition_id: String, pub state: ProcessState, pub current_node_id: Option<String>, pub variables: HashMap<String, serde_json::Value>, pub started_at: DateTime<Utc>, pub updated_at: DateTime<Utc> }

pub struct ProcessInstance { pub id: String, pub definition: Arc<ProcessDefinition>, pub state: Mutex<ProcessState>, pub variables: Mutex<HashMap<String, serde_json::Value>>, pub created_at: DateTime<Utc>, pub updated_at: Mutex<DateTime<Utc>> }

impl ProcessInstance {
    pub fn new(id: impl Into<String>, definition: Arc<ProcessDefinition>) -> Self {
        let mut vars = HashMap::new();
        for v in &definition.variables {
            if let Some(ref dv) = v.default_value { vars.insert(v.name.clone(), dv.clone()); }
            else if v.required { vars.insert(v.name.clone(), serde_json::Value::Null); }
        }
        Self { id: id.into(), definition, state: Mutex::new(ProcessState::Created), variables: Mutex::new(vars), created_at: Utc::now(), updated_at: Mutex::new(Utc::now()) }
    }

    pub fn state(&self) -> ProcessState { self.state.lock().clone() }

    pub fn start(&self) -> rust_agent_core::Result<()> {
        let mut s = self.state.lock();
        if *s == ProcessState::Created || *s == ProcessState::Suspended { *s = ProcessState::Running; *self.updated_at.lock() = Utc::now(); Ok(()) }
        else { Err(rust_agent_core::AgentError::WorkflowError(format!("cannot start from {:?}", *s))) }
    }

    pub fn suspend(&self) -> rust_agent_core::Result<()> {
        let mut s = self.state.lock();
        if *s == ProcessState::Running { *s = ProcessState::Suspended; *self.updated_at.lock() = Utc::now(); Ok(()) }
        else { Err(rust_agent_core::AgentError::WorkflowError(format!("cannot suspend from {:?}", *s))) }
    }

    pub fn resume(&self) -> rust_agent_core::Result<()> {
        let mut s = self.state.lock();
        if *s == ProcessState::Suspended { *s = ProcessState::Running; *self.updated_at.lock() = Utc::now(); Ok(()) }
        else { Err(rust_agent_core::AgentError::WorkflowError(format!("cannot resume from {:?}", *s))) }
    }

    pub fn complete(&self) -> rust_agent_core::Result<()> {
        let mut s = self.state.lock();
        if *s == ProcessState::Running { *s = ProcessState::Completed; *self.updated_at.lock() = Utc::now(); Ok(()) }
        else { Err(rust_agent_core::AgentError::WorkflowError(format!("cannot complete from {:?}", *s))) }
    }

    pub fn terminate(&self, reason: impl Into<String>) -> rust_agent_core::Result<()> {
        let mut s = self.state.lock();
        if *s != ProcessState::Completed && *s != ProcessState::Terminated && *s != ProcessState::Failed { *s = ProcessState::Terminated; tracing::info!(id=%self.id, reason=%reason.into(), "terminated"); *self.updated_at.lock() = Utc::now(); Ok(()) }
        else { Err(rust_agent_core::AgentError::WorkflowError(format!("cannot terminate from {:?}", *s))) }
    }

    pub fn fail(&self, error: impl Into<String>) -> rust_agent_core::Result<()> {
        let mut s = self.state.lock();
        if *s != ProcessState::Completed && *s != ProcessState::Terminated && *s != ProcessState::Failed { *s = ProcessState::Failed; tracing::error!(id=%self.id, error=%error.into(), "failed"); *self.updated_at.lock() = Utc::now(); Ok(()) }
        else { Err(rust_agent_core::AgentError::WorkflowError(format!("cannot fail from {:?}", *s))) }
    }

    pub fn snapshot(&self) -> ProcessSnapshot { ProcessSnapshot { process_id: self.id.clone(), definition_id: self.definition.id.clone(), state: self.state(), current_node_id: None, variables: self.variables.lock().clone(), started_at: self.created_at, updated_at: *self.updated_at.lock() } }
    pub fn get_variable(&self, name: &str) -> Option<serde_json::Value> { self.variables.lock().get(name).cloned() }
    pub fn set_variable(&self, name: &str, value: serde_json::Value) { self.variables.lock().insert(name.to_string(), value); }
}

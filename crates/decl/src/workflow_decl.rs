use serde::{Deserialize, Serialize};

use crate::actions::ActionDecl;

/// Workflow agent data (kind = "workflow").
/// Aligns with MAF AgentSchema v1.0 `Workflow`.
///
/// A workflow agent orchestrates multiple steps and actions. It uses a
/// trigger-based action-list DSL where actions execute sequentially.
/// Supports conditional logic, parallel processing, and sophisticated
/// AI-driven processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowAgentData {
    /// The trigger that initiates the workflow execution.
    pub trigger: WorkflowTrigger,
}

/// The trigger that starts a workflow execution.
/// Aligns with MAF Declarative Workflows trigger structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    /// Trigger type (typically `"OnConversationStart"`).
    pub kind: String,
    /// Unique identifier for the workflow trigger.
    pub id: String,
    /// List of actions to execute when triggered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionDecl>,
}

impl WorkflowAgentData {
    /// Create a new workflow with a trigger.
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            trigger: WorkflowTrigger {
                kind: kind.into(),
                id: id.into(),
                actions: Vec::new(),
            },
        }
    }

    /// Add an action to the trigger's action list.
    pub fn with_action(mut self, action: ActionDecl) -> Self {
        self.trigger.actions.push(action);
        self
    }

    /// Get a reference to all actions.
    pub fn actions(&self) -> &[ActionDecl] {
        &self.trigger.actions
    }
}

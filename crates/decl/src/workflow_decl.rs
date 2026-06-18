use serde::{Deserialize, Serialize};

use crate::actions::ActionDecl;

/// 工作流 Agent 数据（kind = "workflow"），与 MAF AgentSchema v1.0 对齐。
///
/// 工作流 Agent 编排多个步骤和动作，使用基于触发器的动作列表 DSL，
/// 动作按顺序执行。支持条件逻辑、并行处理和复杂的 AI 驱动流程。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowAgentData {
    /// 启动工作流执行的触发器。
    pub trigger: WorkflowTrigger,
}

/// 启动工作流执行的触发器，与 MAF Declarative Workflows 触发器结构对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTrigger {
    /// 触发器类型（通常为 `"OnConversationStart"`）。
    pub kind: String,
    /// 工作流触发器的唯一标识符。
    pub id: String,
    /// 触发时执行的动作列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<ActionDecl>,
}

impl WorkflowAgentData {
    /// 使用触发器创建新工作流。
    pub fn new(kind: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            trigger: WorkflowTrigger {
                kind: kind.into(),
                id: id.into(),
                actions: Vec::new(),
            },
        }
    }

    /// 向触发器的动作列表添加动作。
    pub fn with_action(mut self, action: ActionDecl) -> Self {
        self.trigger.actions.push(action);
        self
    }

    /// 获取所有动作的引用。
    pub fn actions(&self) -> &[ActionDecl] {
        &self.trigger.actions
    }
}

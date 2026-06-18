use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::container_agent::ContainerAgentData;
use crate::prompt_agent::PromptAgentData;
use crate::schema::PropertySchema;
use crate::workflow_decl::WorkflowAgentData;

/// 统一的 Agent 定义类型，与 MAF AgentSchema v1.0 对齐。
///
/// 结构体持有所有 Agent 类型共有的基本字段（名称、描述、元数据、输入/输出模式），
/// 通过 serde flatten 将类型特定数据委托给 `AgentKindData`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Agent 的人类可读名称。
    pub name: String,
    /// 用于 UI 的展示名称。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "displayName")]
    pub display_name: Option<String>,
    /// Agent 能力与用途的描述。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// 附加元数据，包括作者、标签等。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// 参与模板渲染的输入参数。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "inputSchema")]
    pub input_schema: Option<PropertySchema>,
    /// Agent 预期的输出格式与结构。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "outputSchema")]
    pub output_schema: Option<PropertySchema>,

    /// 类型特定数据（prompt、hosted、workflow），
    /// `kind` 字段由内部标签枚举注入。
    #[serde(flatten)]
    pub kind_data: AgentKindData,
}

/// 按 `kind` 字段区分的类型特定 Agent 数据，与 MAF 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentKindData {
    /// 基于提示词的 AI Agent。
    #[serde(rename = "prompt")]
    Prompt(PromptAgentData),
    /// 托管/容器化 Agent。
    #[serde(rename = "hosted")]
    Container(ContainerAgentData),
    /// 工作流编排 Agent。
    #[serde(rename = "workflow")]
    Workflow(WorkflowAgentData),
}

impl AgentDefinition {
    /// 创建一个新的提示词 Agent 定义。
    pub fn new_prompt(name: impl Into<String>, model: crate::model::Model) -> Self {
        Self {
            name: name.into(),
            display_name: None,
            description: String::new(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            kind_data: AgentKindData::Prompt(PromptAgentData::new(model)),
        }
    }

    /// 创建一个新的工作流定义。
    pub fn new_workflow(name: impl Into<String>, trigger_kind: impl Into<String>, trigger_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            display_name: None,
            description: String::new(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            kind_data: AgentKindData::Workflow(WorkflowAgentData::new(trigger_kind, trigger_id)),
        }
    }

    /// 创建一个新的容器/托管 Agent 定义。
    pub fn new_container(name: impl Into<String>, resources: crate::container_agent::ContainerResources) -> Self {
        use crate::container_agent::{ContainerAgentData, ProtocolVersionRecord};
        Self {
            name: name.into(),
            display_name: None,
            description: String::new(),
            metadata: HashMap::new(),
            input_schema: None,
            output_schema: None,
            kind_data: AgentKindData::Container(ContainerAgentData::new(
                vec![ProtocolVersionRecord::new("responses")],
                resources,
            )),
        }
    }

    /// 检查是否为提示词 Agent。
    pub fn is_prompt(&self) -> bool {
        matches!(self.kind_data, AgentKindData::Prompt(_))
    }

    /// 检查是否为工作流 Agent。
    pub fn is_workflow(&self) -> bool {
        matches!(self.kind_data, AgentKindData::Workflow(_))
    }

    /// 检查是否为容器 Agent。
    pub fn is_container(&self) -> bool {
        matches!(self.kind_data, AgentKindData::Container(_))
    }
}

use serde::{Deserialize, Serialize};

use crate::definition::AgentDefinition;
use crate::model::Model;
use crate::template::Template;
use crate::tools::ToolDecl;

fn default_max_tool_rounds() -> usize {
    10
}

/// 提示词 Agent 数据（kind = "prompt"），与 MAF AgentSchema v1.0 对齐。
///
/// 这是最常见的 Agent 类型，支持模型配置、工具注册、基于模板的提示词渲染和指令。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAgentData {
    /// 主 AI 模型配置（MAF 中必需）。
    pub model: Model,

    /// Agent 可用的工具。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDecl>,

    /// 提示词渲染的模板配置。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<Template>,

    /// Agent 的系统指令/提示词。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instructions: String,

    /// Agent 的附加指令或上下文。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "additionalInstructions")]
    pub additional_instructions: Option<String>,

    // ── 扩展字段（非 MAF，框架特有）──

    /// 强制停止前的最大工具调用轮数。
    #[serde(default = "default_max_tool_rounds", skip_serializing_if = "is_default_max_tool_rounds", rename = "maxToolRounds")]
    pub max_tool_rounds: usize,

    /// 嵌套的子 Agent 声明（递归 `AgentDefinition` 条目）。
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "subAgents")]
    pub sub_agents: Vec<AgentDefinition>,
}

fn is_default_max_tool_rounds(v: &usize) -> bool {
    *v == default_max_tool_rounds()
}

impl PromptAgentData {
    /// 用指定模型创建提示词 Agent。
    pub fn new(model: Model) -> Self {
        Self {
            model,
            tools: Vec::new(),
            template: None,
            instructions: String::new(),
            additional_instructions: None,
            max_tool_rounds: default_max_tool_rounds(),
            sub_agents: Vec::new(),
        }
    }

    /// 向 Agent 添加工具。
    pub fn with_tool(mut self, tool: ToolDecl) -> Self {
        self.tools.push(tool);
        self
    }

    /// 设置系统指令。
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = instructions.into();
        self
    }

    /// 添加附加指令。
    pub fn with_additional_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.additional_instructions = Some(instructions.into());
        self
    }

    /// 设置模板配置。
    pub fn with_template(mut self, template: Template) -> Self {
        self.template = Some(template);
        self
    }

    /// 设置最大工具调用轮数。
    pub fn with_max_tool_rounds(mut self, rounds: usize) -> Self {
        self.max_tool_rounds = rounds;
        self
    }

    /// 添加子 Agent。
    pub fn with_sub_agent(mut self, sub_agent: AgentDefinition) -> Self {
        self.sub_agents.push(sub_agent);
        self
    }
}

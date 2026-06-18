//! CompileContext — 编译上下文
//!
//! 在整个编译过程中维护节点 ID 计数器、变量到节点的映射、
//! GotoAction 标签目标及延迟连接队列。

use std::collections::HashMap;

use crate::resolver::agent_resolver::AgentResolver;
use crate::resolver::tool_resolver::ToolResolver;

/// 编译上下文，贯穿 Pass 1 (ActionDecl→IR) 和 Pass 2 (IR→WorkflowGraph)。
pub struct CompileContext<'a> {
    /// 节点 ID 序列计数器
    node_counter: u64,
    /// 变量名 → 产生此变量的节点 ID
    pub variable_nodes: HashMap<String, String>,
    /// 标签（Agent 名/ID） → 节点 ID（GotoAction 目标）
    pub label_targets: HashMap<String, String>,
    /// (source_node_id, target_label) — Pass 2 结束时回填
    pub pending_gotos: Vec<(String, String)>,
    /// 工作流级变量声明列表
    pub workflow_variables: Vec<String>,
    /// 触发类型（如 "OnConversationStart"）
    pub trigger_kind: String,
    /// Agent 解析器引用（用于 InvokeAgent 编译时查找 Agent 实例）
    pub agent_resolver: Option<&'a mut AgentResolver>,
    /// 工具解析器引用（用于 InvokeFunctionTool 编译时解析工具）
    pub tool_resolver: Option<&'a ToolResolver>,
}

impl<'a> CompileContext<'a> {
    pub fn new(trigger_kind: String) -> Self {
        Self {
            node_counter: 0,
            variable_nodes: HashMap::new(),
            label_targets: HashMap::new(),
            pending_gotos: Vec::new(),
            workflow_variables: Vec::new(),
            trigger_kind,
            agent_resolver: None,
            tool_resolver: None,
        }
    }

    /// 生成下一个唯一节点 ID。
    pub fn next_node_id(&mut self, prefix: &str) -> String {
        let id = format!("{}_{}", prefix, self.node_counter);
        self.node_counter += 1;
        id
    }

    /// 注册变量对应的节点。
    pub fn register_variable(&mut self, variable: &str, node_id: &str) {
        self.variable_nodes
            .insert(variable.to_string(), node_id.to_string());
    }

    /// 注册 GotoAction 标签目标。
    pub fn register_label(&mut self, label: &str, node_id: &str) {
        self.label_targets
            .insert(label.to_string(), node_id.to_string());
    }

    /// 添加待回填的 GotoAction 连接。
    pub fn add_pending_goto(&mut self, from_id: &str, to_label: &str) {
        self.pending_gotos
            .push((from_id.to_string(), to_label.to_string()));
    }
}

//! CompileContext — 编译上下文
//!
//! 在整个编译过程中维护节点 ID 计数器、变量到节点的映射、
//! GotoAction 标签目标及延迟连接队列。

use std::collections::HashMap;

/// 编译上下文，贯穿 Pass 1 (ActionDecl→IR) 和 Pass 2 (IR→WorkflowGraph)。
pub struct CompileContext {
    /// 节点 ID 序列计数器
    node_counter: u64,
    /// 变量名 → 产生此变量的节点 ID
    pub variable_nodes: HashMap<String, String>,
    /// 标签（Agent 名/ID） → 节点 ID（GotoAction 目标）
    pub label_targets: HashMap<String, String>,
    /// (source_node_id, target_label) — Pass 2 结束时回填
    pub pending_gotos: Vec<(String, String)>,
}

impl CompileContext {
    pub fn new(_trigger_kind: String) -> Self {
        Self {
            node_counter: 0,
            variable_nodes: HashMap::new(),
            label_targets: HashMap::new(),
            pending_gotos: Vec::new(),
        }
    }

    /// 生成下一个唯一节点 ID。
    pub fn next_node_id(&mut self, prefix: &str) -> String {
        let id = format!("{}_{}", prefix, self.node_counter);
        self.node_counter += 1;
        id
    }
}

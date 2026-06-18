//! CompileNode — ActionDecl 编译中间表示 (IR)
//!
//! 所有 ActionDecl 首先编译为 CompileNode 树，然后再转换为 WorkflowGraph。

use std::collections::HashMap;

/// 编译中间表示节点。
///
/// 表示结构化的工作流节点树，支持顺序、分支、循环等控制流。
#[derive(Debug, Clone)]
pub enum CompileNode {
    /// 原子节点：直接映射为一个 WorkflowGraph Node
    Atomic {
        node_id: String,
        executor_kind: ExecutorKind,
        is_output: bool,
    },
    /// 顺序链：子节点按序串联执行
    Sequential(Vec<CompileNode>),
    /// 条件分支：if/else 各有独立子图
    Branch {
        condition_node_id: String,
        condition: String,
        true_branch: Box<CompileNode>,
        false_branch: Option<Box<CompileNode>>,
    },
    /// 多条件分支（ConditionGroup）：多个条件/子图对 + 默认分支
    MultiBranch {
        condition_node_id: String,
        branches: Vec<(String, CompileNode)>,
        else_branch: Option<Box<CompileNode>>,
    },
    /// 循环：包含循环体子图
    Loop {
        entry_node_id: String,
        source: String,
        item_name: String,
        index_name: String,
        body: Box<CompileNode>,
        max_iterations: usize,
    },
    /// Continue（循环中跳回入口）
    Continue,
    /// 终止：标记工作流/分支结束
    Terminate,
    /// 空操作
    NoOp,
}

/// 执行器种类 — 描述节点在生产 WorkflowGraph 时应使用的执行器。
#[derive(Debug, Clone)]
pub enum ExecutorKind {
    /// AgentExecutor: 调用已注册的 Agent
    Agent(String),
    /// SetVariable: 写单个变量到 state_map
    SetVariable {
        variable: String,
        value: serde_json::Value,
    },
    /// SetMultipleVariables: 批量写入变量
    SetMultipleVariables {
        variables: HashMap<String, serde_json::Value>,
    },
    /// ResetVariable: 清除单个变量
    ResetVariable { variable: String },
    /// ClearAllVariables: 清除所有变量
    ClearAllVariables,
    /// ParseValue: 从 source 读取并写入 target
    ParseValue { source: String, target: String },
    /// EditTableV2: 编辑表格型变量
    EditTable {
        table: String,
        operation: String,
        row: HashMap<String, serde_json::Value>,
    },
    /// SendActivity: 输出活动消息
    SendActivity { text: serde_json::Value },
    /// ToolCall: 直接调用函数工具
    ToolCall {
        function_name: String,
        arguments: HashMap<String, serde_json::Value>,
        output_variable: Option<String>,
    },
    /// HumanTask: 人工审批/输入
    HumanTask(serde_json::Value),
    /// HttpRequest: HTTP 请求
    HttpRequest {
        url: String,
        method: String,
        headers: HashMap<String, String>,
        body: String,
        response_variable: Option<String>,
    },
    /// McpRequest: MCP 服务器工具调用
    McpRequest {
        server_url: String,
        tool_name: String,
        arguments: HashMap<String, serde_json::Value>,
        output_variable: Option<String>,
    },
    /// EndWorkflow: 终止工作流
    EndWorkflow,
    /// CreateConversation: 创建会话
    CreateConversation { conversation_id: String },
    /// AddMessage: 添加对话消息
    AddMessage { role: String, content: String },
    /// NoOp: 无操作
    NoOp,
}

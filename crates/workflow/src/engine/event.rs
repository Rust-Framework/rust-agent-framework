use rust_agent_core::ChatMessage;
use serde::Serialize;

/// 工作流事件 — 全生命周期 + 节点级粒度
///
/// RAF 的可观测性核心。前端可逐事件消费，实现：
/// - DAG 图实时高亮（当前活跃节点）
/// - 每个 Agent 的状态徽标（等待中 / 运行中 / 完成 / 失败）
/// - 每个 Agent 的实时打字机输出
/// - 多 Agent 并行进度条
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event_type", content = "data")]
pub enum WorkflowEvent {
    /// 工作流启动
    WorkflowStarted {
        session_id: String,
        graph_node_ids: Vec<String>,
        start_node_id: String,
    },

    // ── SuperStep 生命周期 ──

    SuperStepStarted {
        step_number: i32,
        active_nodes: Vec<String>,
    },
    SuperStepCompleted {
        step_number: i32,
        outputs_count: usize,
    },

    // ── 节点生命周期 ──

    /// 节点收到消息，即将开始执行
    NodeInvoking {
        node_id: String,
        node_name: String,
        step_number: i32,
    },
    /// 节点流式输出增量 — 最核心的前端交互事件
    NodeStreaming {
        node_id: String,
        chunk: NodeChunk,
    },
    /// 节点执行成功
    NodeCompleted {
        node_id: String,
        messages_produced: usize,
        usage: Option<UsageInfo>,
    },
    /// 节点执行失败
    NodeFailed {
        node_id: String,
        error: String,
    },

    // ── 输出 / 终止 ──

    AgentResponse {
        node_id: String,
        response: ChatMessage,
    },
    WorkflowCompleted {
        total_steps: i32,
        total_nodes: usize,
        total_usage: Option<UsageInfo>,
    },
    WorkflowError {
        error: String,
        node_id: Option<String>,
    },

    // ── 暂停 / 恢复 ──
    /// 工作流因 halt 请求暂停
    WorkflowHalted {
        step_number: i32,
        reason: Option<String>,
    },
    /// 工作流从暂停中恢复
    WorkflowResumed {
        step_number: i32,
    },

    // ── 超时 / 定时器 ──
    /// 整体工作流超时
    WorkflowTimeout {
        elapsed: std::time::Duration,
    },
    /// 定时器触发
    TimerFired {
        node_id: String,
        timer_name: String,
    },

    // ── 自定义事件 ──
    Custom {
        key: String,
        data: serde_json::Value,
    },
}

/// 节点流式块 — 映射 NodeProgress → 可序列化事件
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "chunk_type")]
pub enum NodeChunk {
    TextDelta { delta: String },
    ReasoningDelta { delta: String },
    ToolCallStart { call_id: String, name: String },
    ToolCallArgs { call_id: String, args_delta: String },
    ToolCallEnd { call_id: String },
    ToolResult { call_id: String, result: String },
    UsageUpdate { prompt_tokens: u32, completion_tokens: u32 },
    Custom { key: String, value: serde_json::Value },
}

/// 用量统计
#[derive(Debug, Clone, Serialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

//! `_raf/*` 扩展方法 — ACP 协议扩展，暴露 Agent 发现与子 Agent 树查询能力。
//!
//! 这些方法不属于标准 ACP 规范，而是 Rust Agent Framework 的扩展，允许 IDE
//! 客户端在运行时发现可用的 Agent 及其子 Agent 结构。
//!
//! # 方法清单
//!
//! | 方法 | 请求 | 响应 | 说明 |
//! |------|------|------|------|
//! | `_raf/agent_list` | `AgentListRequest` | `AgentListResponse` | 列出所有已注册 Agent |
//! | `_raf/agent_info` | `AgentInfoRequest` | `AgentInfoResponse` | 查询指定 Agent 详情 |
//! | `_raf/subagent_list` | `SubAgentListRequest` | `SubAgentListResponse` | 列出指定 Agent 的子 Agent |
//! | `_raf/subagent_tree` | `SubAgentTreeRequest` | `SubAgentTreeResponse` | 查询子 Agent 树 |

use serde::{Deserialize, Serialize};
use tracing::debug;

use agent_client_protocol::{
    ConnectionTo, Client, JsonRpcRequest, JsonRpcResponse,
};

use crate::registry::agent_registry::{AgentRegistry, AgentInfo, SubAgentInfo, SubAgentNode};
use rust_agent_core::AgentId;

// ============================================================================
// 1. _raf/agent_list
// ============================================================================

/// `_raf/agent_list` 请求 — 列出所有已注册 Agent。
///
/// 无需参数，返回当前 Host 注册的全部 Agent 信息。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_raf/agent_list", response = AgentListResponse)]
pub struct AgentListRequest {
    /// 可选：按 agent_type 过滤（如 "chat"、"workflow"）。
    /// 为 None 时返回全部 Agent。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
}

/// `_raf/agent_list` 响应。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
pub struct AgentListResponse {
    /// 所有匹配的 Agent 列表。
    pub agents: Vec<AgentInfo>,
    /// Host 版本。
    pub version: String,
}

// ============================================================================
// 2. _raf/agent_info
// ============================================================================

/// `_raf/agent_info` 请求 — 查询指定 Agent 的详细信息。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_raf/agent_info", response = AgentInfoResponse)]
pub struct AgentInfoRequest {
    /// 目标 Agent ID。
    pub agent_id: String,
}

/// `_raf/agent_info` 响应。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
pub struct AgentInfoResponse {
    /// Agent 信息。若 agent_id 不存在则为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentInfo>,
    /// 是否为工作流 Agent（注册在 WorkflowGraphRegistry 中）。
    pub is_workflow: bool,
}

// ============================================================================
// 3. _raf/subagent_list
// ============================================================================

/// `_raf/subagent_list` 请求 — 列出指定 Agent 的全部子 Agent（扁平列表）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_raf/subagent_list", response = SubAgentListResponse)]
pub struct SubAgentListRequest {
    /// 目标 Agent ID。
    pub agent_id: String,
}

/// `_raf/subagent_list` 响应。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
pub struct SubAgentListResponse {
    /// 子 Agent 列表（扁平，包含 depth 信息）。
    pub subagents: Vec<SubAgentInfo>,
}

// ============================================================================
// 4. _raf/subagent_tree
// ============================================================================

/// `_raf/subagent_tree` 请求 — 查询指定 Agent 的子 Agent 树（嵌套结构）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "_raf/subagent_tree", response = SubAgentTreeResponse)]
pub struct SubAgentTreeRequest {
    /// 目标 Agent ID。
    pub agent_id: String,
}

/// `_raf/subagent_tree` 响应。
#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
pub struct SubAgentTreeResponse {
    /// 子 Agent 树根节点。若 agent_id 不存在则为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<SubAgentNode>,
}

// ============================================================================
// Handler functions
// ============================================================================

/// 处理 `_raf/agent_list` 请求。
pub async fn handle_agent_list(
    req: AgentListRequest,
    responder: agent_client_protocol::Responder<AgentListResponse>,
    _conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
) -> agent_client_protocol::Result<()> {
    debug!(agent_type_filter = ?req.agent_type, "Handling _raf/agent_list");

    let mut agents = registry.build_agent_list();

    // 可选过滤
    if let Some(ref filter_type) = req.agent_type {
        agents.retain(|a| a.agent_type == *filter_type);
    }

    let response = AgentListResponse {
        agents,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    responder.respond(response)
}

/// 处理 `_raf/agent_info` 请求。
pub async fn handle_agent_info(
    req: AgentInfoRequest,
    responder: agent_client_protocol::Responder<AgentInfoResponse>,
    _conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
    graph_registry: &crate::handler::workflow_prompt::WorkflowGraphRegistry,
) -> agent_client_protocol::Result<()> {
    debug!(agent_id = %req.agent_id, "Handling _raf/agent_info");

    let agents = registry.build_agent_list();
    let agent = agents.into_iter().find(|a| a.id == req.agent_id);
    let is_workflow = graph_registry.contains(&req.agent_id);

    let response = AgentInfoResponse {
        agent,
        is_workflow,
    };

    responder.respond(response)
}

/// 处理 `_raf/subagent_list` 请求。
pub async fn handle_subagent_list(
    req: SubAgentListRequest,
    responder: agent_client_protocol::Responder<SubAgentListResponse>,
    _conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
) -> agent_client_protocol::Result<()> {
    debug!(agent_id = %req.agent_id, "Handling _raf/subagent_list");

    let subagents = registry.get_subagent_list(&AgentId::new(&req.agent_id));

    let response = SubAgentListResponse { subagents };

    responder.respond(response)
}

/// 处理 `_raf/subagent_tree` 请求。
pub async fn handle_subagent_tree(
    req: SubAgentTreeRequest,
    responder: agent_client_protocol::Responder<SubAgentTreeResponse>,
    _conn: ConnectionTo<Client>,
    registry: &AgentRegistry,
) -> agent_client_protocol::Result<()> {
    debug!(agent_id = %req.agent_id, "Handling _raf/subagent_tree");

    let tree = registry.get_subagent_tree(&AgentId::new(&req.agent_id));

    let response = SubAgentTreeResponse { tree };

    responder.respond(response)
}

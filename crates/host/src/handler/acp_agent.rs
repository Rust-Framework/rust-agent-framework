//! ACP Agent handler — the main Agent.builder() assembly.

use std::sync::Arc;
use tracing::{info, debug};
use serde_json::Value;

use agent_client_protocol::{Client, ConnectionTo};
use agent_client_protocol::schema::{
    InitializeRequest, InitializeResponse, AgentCapabilities, PromptCapabilities,
    SessionCapabilities,
    NewSessionRequest, NewSessionResponse, SessionId,
    PromptRequest,
    CancelNotification,
    DeleteSessionRequest, DeleteSessionResponse,
    CloseSessionRequest, CloseSessionResponse,
};

use crate::registry::agent_registry::AgentRegistry;
use crate::bridge::session::SessionBridge;
use crate::handler::prompt::route_prompt;
use crate::handler::workflow_prompt::WorkflowGraphRegistry;
use crate::handler::raf_extensions;

pub struct RafAgentHost {
    pub registry: Arc<AgentRegistry>,
    pub session_bridge: Arc<SessionBridge>,
    /// 工作流图注册表 — 存储 HITL-capable 工作流 Agent 的图（例如 `dev-pipeline`）。
    pub graph_registry: Arc<tokio::sync::Mutex<WorkflowGraphRegistry>>,
}

impl RafAgentHost {
    pub async fn run(
        self,
        transport: impl agent_client_protocol::ConnectTo<agent_client_protocol::Agent>,
    ) -> agent_client_protocol::Result<()> {
        let registry = self.registry.clone();
        let bridge = self.session_bridge.clone();
        let graph_registry = self.graph_registry.clone();

        info!("Starting ACP agent server");

        let acp_agent = agent_client_protocol::Agent;
        let r1 = registry.clone();

        acp_agent.builder()
            .name("rust-agent-host")
            // 1. Initialize
            .on_receive_request(
                async move |req: InitializeRequest, responder, _conn| {
                    debug!("Initialize request");
                    let caps = AgentCapabilities::new()
                        .prompt_capabilities(PromptCapabilities::new())
                        .session_capabilities(SessionCapabilities::new());
                    let mut resp = InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(caps);
                    let agent_list = r1.build_agent_list_meta();
                    if let Value::Object(map) = agent_list {
                        resp.meta = Some(map);
                    }
                    responder.respond(resp)
                },
                agent_client_protocol::on_receive_request!(),
            )
            // 2. Session creation
            .on_receive_request({
                let b2 = bridge.clone();
                let gr2 = graph_registry.clone();
                async move |req: NewSessionRequest, responder, _conn| {
                    let target_agent = req.meta.as_ref()
                        .and_then(|m| m.get("raf.agent_id"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    // Determine if this is a workflow agent session
                    let is_workflow = if let Some(ref id) = target_agent {
                        let reg = gr2.lock().await;
                        reg.contains(id)
                    } else {
                        false
                    };

                    let sid = uuid::Uuid::new_v4().to_string();
                    let _ = b2.create_session(&sid, target_agent.as_deref(), is_workflow).await;
                    debug!(session_id = %sid, is_workflow, "Session created");
                    responder.respond(NewSessionResponse::new(SessionId::new(sid)))
                }
            }, agent_client_protocol::on_receive_request!())
            // 3. Prompt handling — route between simple agent and workflow paths
            .on_receive_request({
                let r3 = registry.clone();
                let b3 = bridge.clone();
                let gr3 = graph_registry.clone();
                async move |req: PromptRequest, responder, conn: ConnectionTo<Client>| {
                    // Clone `gr3` inside the closure so the closure is `Fn`
                    // (can be called multiple times) rather than `FnOnce`.
                    route_prompt(req, responder, conn, &r3, b3.clone(), gr3.clone()).await
                }
            }, agent_client_protocol::on_receive_request!())
            // 4. Cancel notification
            .on_receive_notification({
                let b4 = bridge.clone();
                async move |notif: CancelNotification, _conn| {
                    let sid = notif.session_id.0.as_ref().to_string();
                    info!(session_id = %sid, "Session cancelled");
                    b4.cancel_session(&sid).await;
                    Ok(())
                }
            }, agent_client_protocol::on_receive_notification!())
            // 4b. session/delete — 删除会话并清理资源
            .on_receive_request({
                let b_del = bridge.clone();
                async move |req: DeleteSessionRequest, responder, _conn| {
                    let sid = req.session_id.0.as_ref().to_string();
                    info!(session_id = %sid, "Session delete requested");
                    b_del.remove_session(&sid).await;
                    responder.respond(DeleteSessionResponse::new())
                }
            }, agent_client_protocol::on_receive_request!())
            // 4c. session/close — 关闭活跃会话
            .on_receive_request({
                let b_close = bridge.clone();
                async move |req: CloseSessionRequest, responder, _conn| {
                    let sid = req.session_id.0.as_ref().to_string();
                    info!(session_id = %sid, "Session close requested");
                    b_close.remove_session(&sid).await;
                    responder.respond(CloseSessionResponse::new())
                }
            }, agent_client_protocol::on_receive_request!())
            // 5. _raf/agent_list — 列出所有已注册 Agent
            .on_receive_request({
                let r5 = registry.clone();
                async move |req: raf_extensions::AgentListRequest, responder, conn| {
                    raf_extensions::handle_agent_list(req, responder, conn, &r5).await
                }
            }, agent_client_protocol::on_receive_request!())
            // 6. _raf/agent_info — 查询指定 Agent 详情
            .on_receive_request({
                let r6 = registry.clone();
                let gr6 = graph_registry.clone();
                async move |req: raf_extensions::AgentInfoRequest, responder, conn| {
                    let gr = gr6.lock().await;
                    raf_extensions::handle_agent_info(req, responder, conn, &r6, &gr).await
                }
            }, agent_client_protocol::on_receive_request!())
            // 7. _raf/subagent_list — 列出指定 Agent 的子 Agent
            .on_receive_request({
                let r7 = registry.clone();
                async move |req: raf_extensions::SubAgentListRequest, responder, conn| {
                    raf_extensions::handle_subagent_list(req, responder, conn, &r7).await
                }
            }, agent_client_protocol::on_receive_request!())
            // 8. _raf/subagent_tree — 查询子 Agent 树
            .on_receive_request({
                let r8 = registry.clone();
                async move |req: raf_extensions::SubAgentTreeRequest, responder, conn| {
                    raf_extensions::handle_subagent_tree(req, responder, conn, &r8).await
                }
            }, agent_client_protocol::on_receive_request!())
            .connect_to(transport)
            .await
    }
}

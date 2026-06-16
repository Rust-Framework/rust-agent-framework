//! ACP Agent handler — the main Agent.builder() assembly.

use std::sync::Arc;
use tracing::{info, debug};
use serde_json::Value;

use agent_client_protocol::{Client, ConnectionTo};
use agent_client_protocol::schema::{
    InitializeRequest, InitializeResponse, AgentCapabilities, PromptCapabilities,
    SessionCapabilities, McpCapabilities,
    NewSessionRequest, NewSessionResponse, SessionId,
    PromptRequest,
    CancelNotification,
};

use crate::registry::agent_registry::AgentRegistry;
use crate::bridge::session::SessionBridge;
use crate::handler::prompt::handle_prompt;

pub struct RafAgentHost {
    pub registry: Arc<AgentRegistry>,
    pub session_bridge: Arc<SessionBridge>,
}

impl RafAgentHost {
    pub async fn run(
        self,
        transport: impl agent_client_protocol::ConnectTo<agent_client_protocol::Agent>,
    ) -> agent_client_protocol::Result<()> {
        let registry = self.registry.clone();
        let bridge = self.session_bridge.clone();

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
                        .session_capabilities(SessionCapabilities::new())
                        .mcp_capabilities(McpCapabilities::new());
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
                async move |req: NewSessionRequest, responder, _conn| {
                    let target_agent = req.meta.as_ref()
                        .and_then(|m| m.get("raf.agent_id"))
                        .and_then(|v| v.as_str());
                    let sid = uuid::Uuid::new_v4().to_string();
                    let _ = b2.create_session(&sid, target_agent).await;
                    debug!(session_id = %sid, "Session created");
                    responder.respond(NewSessionResponse::new(SessionId::new(sid)))
                }
            }, agent_client_protocol::on_receive_request!())
            // 3. Prompt handling
            .on_receive_request({
                let r3 = registry.clone();
                let b3 = bridge.clone();
                async move |req: PromptRequest, responder, conn: ConnectionTo<Client>| {
                    handle_prompt(req, responder, conn, &r3, &b3).await
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
            .connect_to(transport)
            .await
    }
}

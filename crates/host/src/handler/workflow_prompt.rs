//! Workflow prompt handler — bridges ACP `session/prompt` to a `WorkflowRuntime`
//! for HITL-capable agents (e.g., the 6-stage coding dev pipeline).
//!
//! Unlike `handler::prompt` (which calls `IAgent::run()` for simple agents),
//! this module uses `WorkflowRuntime::start()` directly, enabling:
//! - Full workflow event → ACP `SessionUpdate` streaming with node-level tagging
//! - HITL via `session/request_permission` when the workflow halts
//! - Resume with user input after confirmation

use std::sync::Arc;

use tracing::{debug, info, warn};

use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::schema::{
    ContentBlock, PromptRequest, PromptResponse, StopReason,
};

use rust_agent_core::ChatMessage;
use rust_agent_workflow::WorkflowGraph;

use crate::bridge::workflow_bridge::run_workflow_to_completion;

/// A registry of workflow graphs keyed by agent ID.
///
/// Stores `WorkflowGraph` instances for HITL-capable agents. When a `session/prompt`
/// targets an agent in this registry, the graph is cloned (not consumed) and run via
/// `WorkflowRuntime` (instead of `IAgent::run()`).
///
/// # 复用语义
///
/// 与早期版本不同，图不再被 `take()` 消费。每次 `session/prompt` 都会通过
/// `clone_graph()` 获取一份克隆，原始图保留在注册表中供后续会话复用。
/// 这使得同一个工作流 Agent（如 `dev-pipeline`）可被多个会话多次调用。
pub struct WorkflowGraphRegistry {
    graphs: std::collections::HashMap<String, WorkflowGraph>,
}

impl WorkflowGraphRegistry {
    pub fn new() -> Self {
        Self {
            graphs: std::collections::HashMap::new(),
        }
    }

    /// Register a workflow graph under the given agent ID.
    pub fn register(&mut self, agent_id: impl Into<String>, graph: WorkflowGraph) {
        self.graphs.insert(agent_id.into(), graph);
    }

    /// Get a reference to a workflow graph by agent ID.
    pub fn get(&self, agent_id: &str) -> Option<&WorkflowGraph> {
        self.graphs.get(agent_id)
    }

    /// Clone the workflow graph for a new execution, keeping the original for reuse.
    ///
    /// `WorkflowGraph` 实现 `Clone`（节点通过 `Arc<dyn IExecutor>` 共享），
    /// 因此克隆是廉价的——仅复制 HashMap 结构，不复制执行器实例。
    /// 每次执行使用独立的克隆，`WorkflowRuntime` 在其上维护独立的执行状态。
    pub fn clone_graph(&self, agent_id: &str) -> Option<WorkflowGraph> {
        self.graphs.get(agent_id).cloned()
    }

    /// Check if an agent ID is registered as a workflow.
    pub fn contains(&self, agent_id: &str) -> bool {
        self.graphs.contains_key(agent_id)
    }

    /// Number of registered workflows.
    pub fn len(&self) -> usize {
        self.graphs.len()
    }

    /// Is the registry empty?
    pub fn is_empty(&self) -> bool {
        self.graphs.is_empty()
    }
}

impl Default for WorkflowGraphRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle a `session/prompt` request for a workflow-based agent.
///
/// This function:
/// 1. Clones the workflow graph from the registry (original is kept for reuse).
/// 2. Converts the ACP prompt blocks to a `ChatMessage`.
/// 3. Registers a cancel token with the `SessionBridge` so `session/cancel`
///    can interrupt the workflow via `WorkflowRuntime::halt()`.
/// 4. Calls `run_workflow_to_completion` which starts the `WorkflowRuntime`,
///    streams events as ACP notifications, and handles HITL halts.
/// 5. Responds with a `PromptResponse` when the workflow finishes.
///
/// # Arguments
/// - `req`: The ACP prompt request.
/// - `responder`: ACP responder for the final response.
/// - `conn`: ACP client connection (for notifications and permission requests).
/// - `graph_registry`: Registry of workflow graphs (the graph for the target
///   agent will be cloned, not consumed).
/// - `session_bridge`: Session bridge for cancel token registration.
/// - `target_agent_id`: The agent ID to look up in the registry.
pub async fn handle_workflow_prompt(
    req: PromptRequest,
    responder: agent_client_protocol::Responder<PromptResponse>,
    conn: ConnectionTo<Client>,
    graph_registry: Arc<tokio::sync::Mutex<WorkflowGraphRegistry>>,
    session_bridge: Arc<crate::bridge::session::SessionBridge>,
    target_agent_id: String,
) -> agent_client_protocol::Result<()> {
    let session_id = req.session_id.clone();
    let sid_str = session_id.0.as_ref().to_string();
    debug!(session_id = %sid_str, agent_id = %target_agent_id, "Handling workflow prompt");

    // Clone the graph from the registry (original kept for reuse by future sessions)
    let graph = {
        let registry = graph_registry.lock().await;
        match registry.clone_graph(&target_agent_id) {
            Some(g) => g,
            None => {
                warn!(agent_id = %target_agent_id, "Workflow graph not found in registry");
                let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                return Ok(());
            }
        }
    };

    // Convert ACP prompt blocks to a ChatMessage
    let initial_message = convert_blocks_to_message(&req.prompt);
    debug!(message_len = initial_message.content.len(), "Converted prompt to message");

    // Register a cancel token so session/cancel can interrupt the workflow
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    session_bridge.register_cancel_token(&sid_str, cancelled.clone()).await;

    // Spawn the workflow execution in a background task
    tokio::spawn(async move {
        let result = run_workflow_to_completion(
            graph,
            initial_message,
            session_id,
            conn,
            cancelled,
        ).await;

        // Clean up the cancel token
        session_bridge.clear_cancel_token(&sid_str).await;

        info!(
            session_id = %sid_str,
            ?result.stop_reason,
            "Workflow prompt turn completed"
        );

        let _ = responder.respond(PromptResponse::new(result.stop_reason));
    });

    Ok(())
}

/// Convert ACP `ContentBlock`s into a single `ChatMessage`.
fn convert_blocks_to_message(blocks: &[ContentBlock]) -> ChatMessage {
    let mut parts = Vec::new();

    for block in blocks {
        match block {
            ContentBlock::Text(tc) => {
                if !tc.text.is_empty() {
                    parts.push(tc.text.as_str().to_string());
                }
            }
            ContentBlock::ResourceLink(rl) => {
                parts.push(format!("[Reference: {}]", rl.uri));
            }
            ContentBlock::Resource(er) => {
                use agent_client_protocol::schema::EmbeddedResourceResource;
                if let EmbeddedResourceResource::TextResourceContents(tc) = &er.resource {
                    parts.push(format!("[Resource: {}]\n{}", tc.uri, tc.text));
                }
            }
            _ => {}
        }
    }

    let content = parts.join("\n\n");
    ChatMessage::user(content)
}

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::{BoxStream, ISession, Result};
use tokio::sync::broadcast;

use crate::checkpoint::{CheckpointManager, ScopeKey};
use crate::executor::{HandlerResult, NodeProgress};
use crate::graph::WorkflowGraph;

use super::edge_runner::{create_edge_runner, IEdgeRunner};
use super::event::{NodeChunk, UsageInfo, WorkflowEvent};
use super::message_envelope::MessageEnvelope;
use super::step_context::StepContext;

/// 工作流输出
#[derive(Debug)]
pub struct WorkflowOutput {
    pub content: Arc<dyn std::any::Any + Send + Sync>,
    pub source_node_id: String,
}

/// 工作流引擎 — 对应 MAF 的 InProcessRunner
pub struct WorkflowEngine {
    graph: Arc<WorkflowGraph>,
    edge_runners: HashMap<String, Arc<dyn IEdgeRunner>>,
    event_tx: broadcast::Sender<WorkflowEvent>,
    checkpoint_manager: Option<Arc<CheckpointManager>>,
}

impl WorkflowEngine {
    pub fn new(graph: WorkflowGraph) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        let mut edge_runners: HashMap<String, Arc<dyn IEdgeRunner>> = HashMap::new();
        for (source_id, edge_set) in graph.edges() {
            for edge in edge_set {
                let runner = create_edge_runner(edge);
                edge_runners.insert(format!("{}:{}", source_id, edge.edge_id()), Arc::from(runner));
            }
        }

        Self {
            graph: Arc::new(graph),
            edge_runners,
            event_tx,
            checkpoint_manager: None,
        }
    }

    /// 配置检查点管理器，启用工作流故障恢复
    pub fn with_checkpoint_manager(mut self, manager: Arc<CheckpointManager>) -> Self {
        self.checkpoint_manager = Some(manager);
        self
    }

    /// 获取图引用（供外部读取节点信息）
    pub fn graph(&self) -> &Arc<WorkflowGraph> {
        &self.graph
    }

    // ═══ 核心 API ═══

    /// 完整运行，返回事件流 + 输出流
    pub async fn run(
        &self,
        initial_message: Arc<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
    ) -> Result<(
        BoxStream<'static, WorkflowEvent>,
        BoxStream<'static, Result<WorkflowOutput>>,
    )> {
        let event_rx = self.event_tx.subscribe();
        let (output_tx, output_rx) = tokio::sync::mpsc::channel::<Result<WorkflowOutput>>(32);

        let graph = self.graph.clone();
        let event_tx = self.event_tx.clone();
        let edge_runners_map = self.edge_runners.clone();
        let checkpoint_manager = self.checkpoint_manager.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::execute_loop(
                graph,
                edge_runners_map,
                event_tx,
                output_tx,
                initial_message,
                session,
                checkpoint_manager,
            )
            .await
            {
                tracing::error!("工作流执行失败: {:?}", e);
            }
        });

        let event_stream: BoxStream<'static, WorkflowEvent> = Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(event_rx)
                .filter_map(|r| futures_util::future::ready(r.ok())),
        );

        let output_stream: BoxStream<'static, Result<WorkflowOutput>> = Box::pin(
            tokio_stream::wrappers::ReceiverStream::new(output_rx),
        );

        Ok((event_stream, output_stream))
    }

    // ═══ 内部执行循环 ═══

    #[allow(clippy::too_many_arguments)]
    async fn execute_loop(
        graph: Arc<WorkflowGraph>,
        edge_runners_map: HashMap<String, Arc<dyn IEdgeRunner>>,
        event_tx: broadcast::Sender<WorkflowEvent>,
        output_tx: tokio::sync::mpsc::Sender<Result<WorkflowOutput>>,
        initial_message: Arc<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
        checkpoint_manager: Option<Arc<CheckpointManager>>,
    ) -> Result<()> {
        let executor_map: HashMap<String, Arc<dyn crate::executor::IExecutor>> = graph
            .nodes()
            .iter()
            .map(|(id, node)| (id.clone(), node.executor.clone()))
            .collect();

        let graph_fingerprint = compute_graph_fingerprint(&graph);

        tracing::debug!(
            node_count = graph.nodes().len(),
            fingerprint = %graph_fingerprint,
            start_node = %graph.start_node_id(),
            has_checkpoint = checkpoint_manager.is_some(),
            "WorkflowEngine::execute_loop starting"
        );

        let state_map: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>> =
            Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        let session_id = session
            .as_ref()
            .map(|s| s.session_id().to_string())
            .unwrap_or_default();

        if let Some(ref cp) = checkpoint_manager {
            tracing::debug!(session_id = %session_id, fingerprint = %graph_fingerprint, "Checkpoint: create_initial");
            if let Err(e) = cp.create_initial(&session_id, &graph_fingerprint).await {
                tracing::warn!(error = %e, "Failed to create initial checkpoint");
            }
        }

        let node_ids: Vec<String> = graph.nodes().keys().cloned().collect();
        let _ = event_tx.send(WorkflowEvent::WorkflowStarted {
            session_id: session_id.clone(),
            graph_node_ids: node_ids,
            start_node_id: graph.start_node_id().to_string(),
        });

        let envelope = MessageEnvelope::new(
            graph.start_node_id(),
            initial_message,
            crate::executor::TypeTag::new("initial"),
        )
        .with_target(graph.start_node_id());

        let mut step_ctx = StepContext::new(0);
        step_ctx.enqueue(envelope);

        let halt_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut total_steps = 0;
        let mut total_nodes = 0usize;
        let total_prompt = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let total_completion = Arc::new(std::sync::atomic::AtomicU32::new(0));

        while step_ctx.has_messages() {
            if halt_flag.load(std::sync::atomic::Ordering::SeqCst) {
                tracing::info!("Workflow halted by node request");
                break;
            }
            total_steps += 1;
            let current_step_number = step_ctx.step_number;

            let active_nodes = step_ctx.active_nodes();
            tracing::debug!(step = current_step_number, active_node_count = active_nodes.len(), active_nodes = %active_nodes.join(", "), "SuperStep: entering");
            let _ = event_tx.send(WorkflowEvent::SuperStepStarted {
                step_number: current_step_number,
                active_nodes: active_nodes.clone(),
            });

            let mut next_step_ctx = StepContext::new(current_step_number + 1);
            let mut handles = Vec::new();

            for node_id in active_nodes {
                let node = match graph.nodes().get(&node_id) {
                    Some(n) => n.clone(),
                    None => continue,
                };

                // 获取该节点的所有消息（整条队列，不再只取第一条）
                let messages = match step_ctx.dequeue_for(&node_id) {
                    Some(msgs) => msgs,
                    None => continue,
                };

                tracing::debug!(node_id = %node_id, message_count = messages.len(), step = current_step_number, "Node: dispatching messages");
                total_nodes += 1;

                let executor = node.executor.clone();
                let event_tx_clone = event_tx.clone();
                let output_tx_clone = output_tx.clone();
                let session_clone = session.clone();
                let node_label = node_id.clone();
                let state_map_clone = state_map.clone();
                let halt_flag_clone = halt_flag.clone();
                let total_prompt_clone = total_prompt.clone();
                let total_completion_clone = total_completion.clone();

                let handle = tokio::spawn(async move {
                    let _ = event_tx_clone.send(WorkflowEvent::NodeInvoking {
                        node_id: node_label.clone(),
                        node_name: node_label.clone(),
                        step_number: current_step_number,
                    });

                    // 🔧 P0 修复：处理该节点的所有消息，汇总结果
                    // 引擎在此调用 IExecutor 的生命周期钩子
                    let _ = executor.on_delivery_start(&EngineWorkContext::stub(&node_label)).await;

                    let mut all_produced: Vec<Arc<dyn std::any::Any + Send + Sync>> = Vec::new();
                    let mut any_output = false;

                    for env in messages {
                        let (progress_tx, mut progress_rx) =
                            tokio::sync::mpsc::unbounded_channel::<NodeProgress>();

                        let event_tx_progress = event_tx_clone.clone();
                        let nid = node_label.clone();
                        let prompt_counter = total_prompt_clone.clone();
                        let completion_counter = total_completion_clone.clone();
                        tokio::spawn(async move {
                            while let Some(progress) = progress_rx.recv().await {
                                if let NodeProgress::UsageUpdate {
                                    prompt_tokens,
                                    completion_tokens,
                                } = &progress
                                {
                                    prompt_counter.fetch_add(*prompt_tokens, std::sync::atomic::Ordering::Relaxed);
                                    completion_counter.fetch_add(*completion_tokens, std::sync::atomic::Ordering::Relaxed);
                                }
                                let chunk = node_progress_to_chunk(progress);
                                let _ = event_tx_progress.send(WorkflowEvent::NodeStreaming {
                                    node_id: nid.clone(),
                                    chunk,
                                });
                            }
                        });

                        let queued_msgs: Arc<parking_lot::Mutex<Vec<Arc<dyn std::any::Any + Send + Sync>>>> =
                            Arc::new(parking_lot::Mutex::new(Vec::new()));
                        let queued_clone = queued_msgs.clone();

                        let work_ctx = EngineWorkContext {
                            node_id: node_label.clone(),
                            session: session_clone.clone(),
                            state_map: state_map_clone.clone(),
                            queued_messages: queued_msgs,
                            output_tx: output_tx_clone.clone(),
                            event_tx: event_tx_clone.clone(),
                            halt_flag: halt_flag_clone.clone(),
                        };

                        match executor.handle(env.content, &work_ctx, progress_tx).await {
                            Ok(result) => {
                                // 🔧 P0 修复：HandlerResult::Output 不再丢弃内容
                                match result {
                                    HandlerResult::Messages(mut msgs) => {
                                        // 合并 ctx.send_message() 排队消息
                                        let mut queued = queued_clone.lock();
                                        msgs.extend(queued.drain(..).collect::<Vec<_>>());
                                        all_produced.extend(msgs);
                                    }
                                    HandlerResult::Output(output) => {
                                        // 直接 yield 输出到外部
                                        let _ = output_tx_clone
                                            .send(Ok(WorkflowOutput {
                                                content: output,
                                                source_node_id: node_label.clone(),
                                            }))
                                            .await;
                                        any_output = true;
                                    }
                                    HandlerResult::None => {
                                        // 仍需要检查 ctx 排队的消息
                                        let mut queued = queued_clone.lock();
                                        all_produced.extend(queued.drain(..).collect::<Vec<_>>());
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = event_tx_clone.send(WorkflowEvent::NodeFailed {
                                    node_id: node_label.clone(),
                                    error: e.to_string(),
                                });
                                let _ = executor.on_delivery_end(&EngineWorkContext::stub(&node_label)).await;
                                return Err(e);
                            }
                        }
                    }

                    let _ = executor.on_delivery_end(&EngineWorkContext::stub(&node_label)).await;

                    let msg_count = all_produced.len();
                    let _ = event_tx_clone.send(WorkflowEvent::NodeCompleted {
                        node_id: node_label.clone(),
                        messages_produced: msg_count,
                        usage: None,
                    });

                    Ok((node_label, all_produced, any_output))
                });

                handles.push(handle);
            }

            // 等待所有节点完成并路由消息
            let mut routed_total = 0usize;
            for handle in handles {
                match handle.await {
                    Ok(Ok((source_node_id, messages, _any_output))) => {
                        tracing::debug!(node_id = %source_node_id, output_message_count = messages.len(), "Node: completed");
                        for msg in messages {
                            // 🔧 使用 Arc 数据构造 envelope — Clone 时零拷贝
                            let type_name = std::any::type_name_of_val(msg.as_ref());
                            let env = MessageEnvelope::new(
                                &source_node_id,
                                msg,
                                crate::executor::TypeTag::new(type_name),
                            );

                            let prefix = format!("{}:", source_node_id);
                            for (_key, runner) in
                                edge_runners_map.iter().filter(|(k, _)| k.starts_with(&prefix))
                            {
                                if let Ok(deliveries) = runner.chase(&env, &executor_map).await {
                                    for delivery in deliveries {
                                        let mut routed_env = delivery.envelope;
                                        routed_env.target_node_id =
                                            Some(delivery.target_node_id.clone());
                                        tracing::debug!(source = %source_node_id, target = %delivery.target_node_id, "Edge: message routed");
                                        next_step_ctx.enqueue(routed_env);
                                        routed_total += 1;
                                    }
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        let _ = event_tx.send(WorkflowEvent::WorkflowError {
                            error: e.to_string(),
                            node_id: None,
                        });
                        return Err(e);
                    }
                    Err(join_err) => {
                        let _ = event_tx.send(WorkflowEvent::WorkflowError {
                            error: format!("节点任务 panic: {}", join_err),
                            node_id: None,
                        });
                        return Err(rust_agent_core::AgentError::WorkflowError(
                            join_err.to_string(),
                        ));
                    }
                }
            }

            let _ = event_tx.send(WorkflowEvent::SuperStepCompleted {
                step_number: current_step_number,
                outputs_count: routed_total,
            });

            tracing::debug!(step = current_step_number, nodes_processed = total_nodes, messages_routed = routed_total, next_step_messages = next_step_ctx.message_count(), "SuperStep: completed");

            // checkpoint: 提交当前 step 的状态快照
            if let Some(ref cp) = checkpoint_manager {
                let current_state: HashMap<String, serde_json::Value> =
                    state_map.lock().await.clone();
                let scope_state: HashMap<ScopeKey, serde_json::Value> = current_state
                    .into_iter()
                    .map(|(k, v)| (ScopeKey::private(&k), v))
                    .collect();

                let mut edge_states: HashMap<String, serde_json::Value> = HashMap::new();
                for (key, runner) in &edge_runners_map {
                    for (k, v) in runner.checkpoint_state() {
                        edge_states.insert(format!("{}:{}", key, k), v);
                    }
                }

                let _ = cp
                    .commit(
                        &session_id,
                        &graph_fingerprint,
                        scope_state,
                        edge_states,
                        Vec::new(),
                        current_step_number,
                    )
                    .await;
            }

            step_ctx = next_step_ctx;
        }

        let _ = event_tx.send(WorkflowEvent::WorkflowCompleted {
            total_steps,
            total_nodes,
            total_usage: Some(UsageInfo {
                prompt_tokens: total_prompt.load(std::sync::atomic::Ordering::Relaxed),
                completion_tokens: total_completion.load(std::sync::atomic::Ordering::Relaxed),
                total_tokens: total_prompt.load(std::sync::atomic::Ordering::Relaxed)
                    + total_completion.load(std::sync::atomic::Ordering::Relaxed),
            }),
        });

        tracing::info!(total_steps, total_nodes, session_id = %session_id, "WorkflowEngine::execute_loop completed");

        Ok(())
    }
}

fn node_progress_to_chunk(progress: NodeProgress) -> NodeChunk {
    match progress {
        NodeProgress::TextDelta(delta) => NodeChunk::TextDelta { delta },
        NodeProgress::ReasoningDelta(delta) => NodeChunk::ReasoningDelta { delta },
        NodeProgress::ToolCallStart { call_id, name } => NodeChunk::ToolCallStart { call_id, name },
        NodeProgress::ToolCallArgs { call_id, args_delta } => {
            NodeChunk::ToolCallArgs { call_id, args_delta }
        }
        NodeProgress::ToolCallEnd { call_id } => NodeChunk::ToolCallEnd { call_id },
        NodeProgress::ToolResult { call_id, result } => NodeChunk::ToolResult { call_id, result },
        NodeProgress::UsageUpdate { prompt_tokens, completion_tokens } => {
            NodeChunk::UsageUpdate { prompt_tokens, completion_tokens }
        }
        NodeProgress::Custom { key, value } => NodeChunk::Custom { key, value },
    }
}

/// 计算图结构指纹 — 使用 SHA-256 前 16 字符确保跨平台稳定
fn compute_graph_fingerprint(graph: &WorkflowGraph) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut node_ids: Vec<&String> = graph.nodes().keys().collect();
    node_ids.sort();
    for id in node_ids {
        id.hash(&mut hasher);
    }
    graph.start_node_id().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 引擎内部的 WorkContext 实现
struct EngineWorkContext {
    node_id: String,
    session: Option<Arc<dyn ISession>>,
    state_map: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    queued_messages: Arc<parking_lot::Mutex<Vec<Arc<dyn std::any::Any + Send + Sync>>>>,
    output_tx: tokio::sync::mpsc::Sender<Result<WorkflowOutput>>,
    event_tx: tokio::sync::broadcast::Sender<WorkflowEvent>,
    halt_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl EngineWorkContext {
    /// 创建一个 stub context 用于生命周期钩子调用
    fn stub(node_id: &str) -> Self {
        let (dummy_tx, _) = tokio::sync::mpsc::channel::<Result<WorkflowOutput>>(1);
        let (dummy_evt_tx, _) = broadcast::channel::<WorkflowEvent>(1);
        EngineWorkContext {
            node_id: node_id.to_string(),
            session: None,
            state_map: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            queued_messages: Arc::new(parking_lot::Mutex::new(Vec::new())),
            output_tx: dummy_tx,
            event_tx: dummy_evt_tx,
            halt_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

#[async_trait::async_trait]
impl crate::engine::IWorkflowContext for EngineWorkContext {
    async fn send_message(&self, envelope: MessageEnvelope) -> Result<()> {
        tracing::debug!(
            "Node {} send_message: {} -> {}",
            self.node_id,
            envelope.message_id,
            envelope.target_node_id.as_deref().unwrap_or("(none)")
        );
        self.queued_messages.lock().push(envelope.content);
        Ok(())
    }

    async fn yield_output(&self, output: Arc<dyn std::any::Any + Send + Sync>) -> Result<()> {
        tracing::debug!("Node {} yield_output", self.node_id);
        let _ = self.output_tx
            .send(Ok(WorkflowOutput {
                content: output,
                source_node_id: self.node_id.clone(),
            }))
            .await;
        Ok(())
    }

    async fn emit_event(&self, event: WorkflowEvent) {
        let _ = self.event_tx.send(event);
    }

    async fn request_halt(&self) {
        tracing::debug!("Node {} request_halt", self.node_id);
        self.halt_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn read_state(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let state = self.state_map.lock().await;
        Ok(state.get(key).cloned())
    }

    async fn write_state(&self, key: &str, value: serde_json::Value) -> Result<()> {
        let mut state = self.state_map.lock().await;
        state.insert(key.to_string(), value);
        Ok(())
    }

    async fn clear_state(&self, key: &str) -> Result<()> {
        let mut state = self.state_map.lock().await;
        state.remove(key);
        Ok(())
    }

    fn current_node_id(&self) -> &str {
        &self.node_id
    }

    fn session(&self) -> Option<&Arc<dyn ISession>> {
        self.session.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::WorkflowBuilder;
    use crate::checkpoint::CheckpointManager;
    use crate::checkpoint::store::InMemoryCheckpointStore;
    use crate::executor::FunctionExecutor;
    use rust_agent_core::AgentSession;
    use std::sync::Once;

    static INIT: Once = Once::new();

    fn init_tracing() {
        INIT.call_once(|| {
            let _ = tracing_subscriber::fmt()
                .with_env_filter("trace")
                .with_test_writer()
                .try_init();
        });
    }

    #[tokio::test]
    async fn test_checkpoint_create_initial_and_commit_timing() {
        init_tracing();

        let builder = WorkflowBuilder::new()
            .add_node("entry", Arc::new(FunctionExecutor::new("entry", |msg: String| {
                vec![format!("processed: {}", msg)]
            })))
            .set_start("entry")
            .with_output_from("entry");

        let graph = builder.build().expect("should build graph");

        let store = Arc::new(InMemoryCheckpointStore::new());
        let cp_manager = Arc::new(CheckpointManager::with_default_config(store));

        let engine = WorkflowEngine::new(graph)
            .with_checkpoint_manager(cp_manager);

        let session: Arc<dyn ISession> = Arc::new(AgentSession::with_id("test-session"));

        let (mut events, _outputs) = engine
            .run(Arc::new("hello".to_string()), Some(session))
            .await
            .expect("should start engine");

        let mut event_count = 0;
        let timeout = tokio::time::sleep(std::time::Duration::from_secs(2));
        tokio::pin!(timeout);
        loop {
            tokio::select! {
                Some(_) = events.next() => { event_count += 1; }
                _ = &mut timeout => { break; }
            }
        }

        assert!(event_count > 0, "Should produce workflow events");
    }
}

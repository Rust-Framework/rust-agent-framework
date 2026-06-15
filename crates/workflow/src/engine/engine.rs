use std::collections::HashMap;
use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::{BoxStream, ISession, Result};
use tokio::sync::broadcast;

use crate::executor::{HandlerResult, NodeProgress};
use crate::graph::WorkflowGraph;

use super::edge_runner::{create_edge_runner, IEdgeRunner};
use super::event::{NodeChunk, WorkflowEvent};
use super::message_envelope::MessageEnvelope;
use super::step_context::StepContext;

/// 工作流输出
#[derive(Debug)]
pub struct WorkflowOutput {
    pub content: Box<dyn std::any::Any + Send + Sync>,
    pub source_node_id: String,
}

/// 工作流引擎 — 对应 MAF 的 InProcessRunner
pub struct WorkflowEngine {
    graph: Arc<WorkflowGraph>,
    edge_runners: HashMap<String, Arc<dyn IEdgeRunner>>,
    event_tx: broadcast::Sender<WorkflowEvent>,
}

impl WorkflowEngine {
    pub fn new(graph: WorkflowGraph) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        // 构建边执行器
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
        }
    }

    // ═══ 核心 API ═══

    /// 完整运行，返回事件流 + 输出流
    pub async fn run(
        &self,
        initial_message: Box<dyn std::any::Any + Send + Sync>,
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

        // 后台执行
        tokio::spawn(async move {
            if let Err(e) = Self::execute_loop(
                graph,
                edge_runners_map,
                event_tx,
                output_tx,
                initial_message,
                session,
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

    /// 订阅事件流（只读，不驱动执行）
    pub fn subscribe_events(&self) -> BoxStream<'static, WorkflowEvent> {
        let rx = self.event_tx.subscribe();
        Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(rx)
                .filter_map(|r| futures_util::future::ready(r.ok())),
        )
    }

    // ═══ 内部执行循环 ═══

    #[allow(clippy::too_many_arguments)]
    async fn execute_loop(
        graph: Arc<WorkflowGraph>,
        edge_runners_map: HashMap<String, Arc<dyn IEdgeRunner>>,
        event_tx: broadcast::Sender<WorkflowEvent>,
        output_tx: tokio::sync::mpsc::Sender<Result<WorkflowOutput>>,
        initial_message: Box<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
    ) -> Result<()> {
        // 构建 executor map（用于 edge_runner chase）
        let executor_map: HashMap<String, Arc<dyn crate::executor::IExecutor>> = graph
            .nodes()
            .iter()
            .map(|(id, node)| (id.clone(), node.executor.clone()))
            .collect();

        // 发送 WorkflowStarted 事件
        let node_ids: Vec<String> = graph.nodes().keys().cloned().collect();
        let _ = event_tx.send(WorkflowEvent::WorkflowStarted {
            session_id: session
                .as_ref()
                .map(|s| s.session_id().to_string())
                .unwrap_or_default(),
            graph_node_ids: node_ids,
            start_node_id: graph.start_node_id().to_string(),
        });

        // 创建初始 StepContext
        let envelope = MessageEnvelope::new(
            graph.start_node_id(),
            initial_message,
            crate::executor::TypeTag::new("initial"),
        )
        .with_target(graph.start_node_id());

        let mut step_ctx = StepContext::new(0);
        step_ctx.enqueue(envelope);

        let mut total_steps = 0;
        let mut total_nodes = 0usize;

        while step_ctx.has_messages() {
            total_steps += 1;
            let current_step_number = step_ctx.step_number;

            let active_nodes = step_ctx.active_nodes();
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

                let messages = match step_ctx.dequeue_for(&node_id) {
                    Some(msgs) => msgs,
                    None => continue,
                };

                total_nodes += 1;

                let executor = node.executor.clone();
                let event_tx_clone = event_tx.clone();
                let output_tx_clone = output_tx.clone();
                let session_clone = session.clone();
                let node_label = node_id.clone();

                let handle = tokio::spawn(async move {
                    let _ = event_tx_clone.send(WorkflowEvent::NodeInvoking {
                        node_id: node_label.clone(),
                        node_name: node_label.clone(),
                        step_number: current_step_number,
                    });

                    for env in messages {
                        let (progress_tx, mut progress_rx) =
                            tokio::sync::mpsc::unbounded_channel::<NodeProgress>();

                        // 进度转发
                        let event_tx_progress = event_tx_clone.clone();
                        let nid = node_label.clone();
                        tokio::spawn(async move {
                            while let Some(progress) = progress_rx.recv().await {
                                let chunk = node_progress_to_chunk(progress);
                                let _ = event_tx_progress.send(WorkflowEvent::NodeStreaming {
                                    node_id: nid.clone(),
                                    chunk,
                                });
                            }
                        });

                        let work_ctx = EngineWorkContext {
                            node_id: node_label.clone(),
                            session: session_clone.clone(),
                        };

                        match executor.handle(env.content, &work_ctx, progress_tx).await {
                            Ok(result) => {
                                let msg_count = match &result {
                                    HandlerResult::Messages(msgs) => msgs.len(),
                                    _ => 0,
                                };

                                let _ = event_tx_clone.send(WorkflowEvent::NodeCompleted {
                                    node_id: node_label.clone(),
                                    messages_produced: msg_count,
                                    usage: None,
                                });

                                match result {
                                    HandlerResult::Messages(msgs) => {
                                        return Ok((node_label.clone(), msgs, node.is_output));
                                    }
                                    HandlerResult::Output(output) => {
                                        if node.is_output {
                                            let _ = output_tx_clone
                                                .send(Ok(WorkflowOutput {
                                                    content: output,
                                                    source_node_id: node_label.clone(),
                                                }))
                                                .await;
                                        }
                                        return Ok((node_label, vec![], false));
                                    }
                                    HandlerResult::None => {
                                        return Ok((node_label, vec![], false));
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = event_tx_clone.send(WorkflowEvent::NodeFailed {
                                    node_id: node_label.clone(),
                                    error: e.to_string(),
                                });
                                return Err(e);
                            }
                        }
                    }

                    Ok((node_label, vec![], false))
                });

                handles.push(handle);
            }

            // 等待所有节点完成并路由消息
            for handle in handles {
                match handle.await {
                    Ok(Ok((source_node_id, messages, _is_output))) => {
                        for msg in messages {
                            let type_name = std::any::type_name_of_val(&msg);
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
                                            Some(delivery.target_node_id);
                                        next_step_ctx.enqueue(routed_env);
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
                outputs_count: 0,
            });

            step_ctx = next_step_ctx;
        }

        let _ = event_tx.send(WorkflowEvent::WorkflowCompleted {
            total_steps,
            total_nodes,
            total_usage: None,
        });

        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn graph(&self) -> &Arc<WorkflowGraph> {
        &self.graph
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
        NodeProgress::UsageUpdate {
            prompt_tokens,
            completion_tokens,
        } => NodeChunk::UsageUpdate {
            prompt_tokens,
            completion_tokens,
        },
        NodeProgress::Custom { key, value } => NodeChunk::Custom { key, value },
    }
}

/// 引擎内部的 WorkContext 实现
struct EngineWorkContext {
    node_id: String,
    session: Option<Arc<dyn ISession>>,
}

#[async_trait::async_trait]
impl crate::engine::IWorkflowContext for EngineWorkContext {
    async fn send_message(&self, envelope: MessageEnvelope) -> Result<()> {
        tracing::debug!("Node {} send_message: {:?}", self.node_id, envelope.message_id);
        Ok(())
    }

    async fn yield_output(&self, _output: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        tracing::debug!("Node {} yield_output", self.node_id);
        Ok(())
    }

    async fn emit_event(&self, _event: WorkflowEvent) {}

    async fn request_halt(&self) {
        tracing::debug!("Node {} request_halt", self.node_id);
    }

    async fn read_state(&self, _key: &str) -> Result<Option<serde_json::Value>> {
        Ok(None)
    }

    async fn write_state(&self, _key: &str, _value: serde_json::Value) -> Result<()> {
        Ok(())
    }

    async fn clear_state(&self, _key: &str) -> Result<()> {
        Ok(())
    }

    fn current_node_id(&self) -> &str {
        &self.node_id
    }

    fn session(&self) -> Option<&Arc<dyn ISession>> {
        self.session.as_ref()
    }
}

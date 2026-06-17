use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rust_agent_core::{AgentError, BoxStream, ISession, Result};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Semaphore;

use crate::checkpoint::{CheckpointManager, ScopeKey};
use crate::executor::{HandlerResult, NodeProgress};
use crate::graph::WorkflowGraph;

use super::config::WorkflowConfig;
use super::edge_runner::{create_edge_runner, IEdgeRunner};
use super::event::{NodeChunk, UsageInfo, WorkflowEvent};
use super::message_envelope::MessageEnvelope;
use super::retry::{ExhaustedAction, RetryConfig};
use super::runtime::ResumeCommand;
use super::step_context::StepContext;

/// 工作流输出
#[derive(Debug)]
pub struct WorkflowOutput {
    pub content: Arc<dyn std::any::Any + Send + Sync>,
    pub source_node_id: String,
}

/// 定时器条目 — 在 SuperStep 循环中 poll
#[derive(Debug, Clone)]
struct TimerEntry {
    node_id: String,
    timer_name: String,
    fires_at: Instant,
}

/// 工作流引擎 — 对应 MAF 的 InProcessRunner
pub struct WorkflowEngine {
    graph: Arc<WorkflowGraph>,
    edge_runners: HashMap<String, Arc<dyn IEdgeRunner>>,
    event_tx: broadcast::Sender<WorkflowEvent>,
    checkpoint_manager: Option<Arc<CheckpointManager>>,
    config: WorkflowConfig,
}

impl WorkflowEngine {
    pub fn new(graph: WorkflowGraph) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        let mut edge_runners: HashMap<String, Arc<dyn IEdgeRunner>> = HashMap::new();
        for (source_id, edge_set) in graph.edges() {
            for edge in edge_set {
                let runner = create_edge_runner(edge);
                edge_runners.insert(
                    format!("{}:{}", source_id, edge.edge_id()),
                    Arc::from(runner),
                );
            }
        }

        Self {
            graph: Arc::new(graph),
            edge_runners,
            event_tx,
            checkpoint_manager: None,
            config: WorkflowConfig::default(),
        }
    }

    pub fn with_checkpoint_manager(mut self, manager: Arc<CheckpointManager>) -> Self {
        self.checkpoint_manager = Some(manager);
        self
    }

    pub fn with_config(mut self, config: WorkflowConfig) -> Self {
        self.config = config;
        self
    }

    pub fn graph(&self) -> &Arc<WorkflowGraph> {
        &self.graph
    }

    pub fn config(&self) -> &WorkflowConfig {
        &self.config
    }

    /// 完整运行，返回事件流 + 输出流（简单场景，不支持 resume）
    pub async fn run(
        &self,
        initial_message: Arc<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
    ) -> Result<(
        BoxStream<'static, WorkflowEvent>,
        BoxStream<'static, Result<WorkflowOutput>>,
    )> {
        self.spawn_run(initial_message, session, None, None).await
    }

    /// 可恢复运行 — 供 WorkflowRuntime 使用
    pub(crate) async fn spawn_run(
        &self,
        initial_message: Arc<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
        resume_rx: Option<UnboundedReceiver<ResumeCommand>>,
        done_tx: Option<tokio::sync::oneshot::Sender<Result<()>>>,
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
        let config = self.config.clone();
        let global_timeout = config.global_timeout;

        tokio::spawn(async move {
            let loop_future = Self::execute_loop(
                graph,
                edge_runners_map,
                event_tx.clone(),
                output_tx,
                initial_message,
                session,
                checkpoint_manager,
                config,
                resume_rx,
            );

            let result = match global_timeout {
                Some(timeout) => {
                    let started = Instant::now();
                    match tokio::time::timeout(timeout, loop_future).await {
                        Ok(result) => {
                            if let Err(ref e) = result {
                                tracing::error!("工作流执行失败: {:?}", e);
                            }
                            result
                        }
                        Err(_) => {
                            let _ = event_tx.send(WorkflowEvent::WorkflowTimeout {
                                elapsed: started.elapsed(),
                            });
                            tracing::warn!("工作流整体超时");
                            Err(AgentError::WorkflowError("工作流整体超时".into()))
                        }
                    }
                }
                None => loop_future.await,
            };

            if let Some(tx) = done_tx {
                let _ = tx.send(result);
            }
        });

        let event_stream: BoxStream<'static, WorkflowEvent> = Box::pin(
            tokio_stream::wrappers::BroadcastStream::new(event_rx)
                .filter_map(|r| futures_util::future::ready(r.ok())),
        );

        let output_stream: BoxStream<'static, Result<WorkflowOutput>> =
            Box::pin(tokio_stream::wrappers::ReceiverStream::new(output_rx));

        Ok((event_stream, output_stream))
    }

    // ══ 内部执行循环 ══

    #[allow(clippy::too_many_arguments)]
    async fn execute_loop(
        graph: Arc<WorkflowGraph>,
        edge_runners_map: HashMap<String, Arc<dyn IEdgeRunner>>,
        event_tx: broadcast::Sender<WorkflowEvent>,
        output_tx: tokio::sync::mpsc::Sender<Result<WorkflowOutput>>,
        initial_message: Arc<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
        checkpoint_manager: Option<Arc<CheckpointManager>>,
        config: WorkflowConfig,
        mut resume_rx: Option<UnboundedReceiver<ResumeCommand>>,
    ) -> Result<()> {
        let executor_map: HashMap<String, Arc<dyn crate::executor::IExecutor>> = graph
            .nodes()
            .iter()
            .map(|(id, node)| (id.clone(), node.executor.clone()))
            .collect();

        let graph_fingerprint = compute_graph_fingerprint(&graph);

        let state_map: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>> =
            Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let timers: Arc<tokio::sync::Mutex<Vec<TimerEntry>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let execution_log: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));

        let session_id = session
            .as_ref()
            .map(|s| s.session_id().to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        if let Some(ref cp) = checkpoint_manager {
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
        let mut total_steps = 0i32;
        let mut total_nodes = 0usize;
        let total_prompt = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let total_completion = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let semaphore = if config.max_parallel_nodes > 0 {
            Some(Arc::new(Semaphore::new(config.max_parallel_nodes)))
        } else {
            None
        };

        let halt_announced = Arc::new(std::sync::atomic::AtomicBool::new(false));

        loop {
            // ── 暂停 / 恢复处理 ──
            if halt_flag.load(std::sync::atomic::Ordering::SeqCst) {
                if !halt_announced.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    save_checkpoint(
                        checkpoint_manager.as_ref(),
                        &session_id,
                        &graph_fingerprint,
                        &state_map,
                        &edge_runners_map,
                        &step_ctx,
                        step_ctx.step_number,
                    )
                    .await;
                    let _ = event_tx.send(WorkflowEvent::WorkflowHalted {
                        step_number: step_ctx.step_number,
                        reason: None,
                    });
                }

                if let Some(ref mut rx) = resume_rx {
                    match rx.recv().await {
                        Some(ResumeCommand::InjectMessage {
                            target_node_id,
                            message,
                        }) => {
                            let env = MessageEnvelope::new(
                                "external",
                                message,
                                crate::executor::TypeTag::new("resume"),
                            )
                            .with_target(target_node_id);
                            step_ctx.enqueue(env);
                            halt_flag.store(false, std::sync::atomic::Ordering::SeqCst);
                            halt_announced.store(false, std::sync::atomic::Ordering::SeqCst);
                            let _ = event_tx.send(WorkflowEvent::WorkflowResumed {
                                step_number: step_ctx.step_number,
                            });
                            continue;
                        }
                        Some(ResumeCommand::Continue) => {
                            halt_flag.store(false, std::sync::atomic::Ordering::SeqCst);
                            halt_announced.store(false, std::sync::atomic::Ordering::SeqCst);
                            let _ = event_tx.send(WorkflowEvent::WorkflowResumed {
                                step_number: step_ctx.step_number,
                            });
                            continue;
                        }
                        Some(ResumeCommand::Abort) => {
                            let _ = event_tx.send(WorkflowEvent::WorkflowError {
                                error: "工作流被外部中止".into(),
                                node_id: None,
                            });
                            return Err(AgentError::WorkflowError("工作流被外部中止".into()));
                        }
                        None => break,
                    }
                } else {
                    break;
                }
            }

            // ── 定时器检查 ──
            fire_due_timers(
                &timers,
                &executor_map,
                &event_tx,
                &mut step_ctx,
            )
            .await;

            if !step_ctx.has_messages() {
                break;
            }

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
                execution_log.lock().await.push(node_id.clone());

                let executor = node.executor.clone();
                let event_tx_clone = event_tx.clone();
                let output_tx_clone = output_tx.clone();
                let session_clone = session.clone();
                let node_label = node_id.clone();
                let state_map_clone = state_map.clone();
                let halt_flag_clone = halt_flag.clone();
                let total_prompt_clone = total_prompt.clone();
                let total_completion_clone = total_completion.clone();
                let timers_clone = timers.clone();
                let retry_config = node.retry.clone();
                let node_timeout = node
                    .timeout
                    .or(config.default_node_timeout);
                let sem = semaphore.clone();

                let handle = tokio::spawn(async move {
                    let _permit = match sem {
                        Some(s) => Some(s.acquire_owned().await.map_err(|e| {
                            AgentError::WorkflowError(format!("并发许可获取失败: {}", e))
                        })?),
                        None => None,
                    };

                    let _ = event_tx_clone.send(WorkflowEvent::NodeInvoking {
                        node_id: node_label.clone(),
                        node_name: node_label.clone(),
                        step_number: current_step_number,
                    });

                    let _ = executor
                        .on_delivery_start(&EngineWorkContext::stub(&node_label))
                        .await;

                    let result = execute_node_messages(
                        &executor,
                        &node_label,
                        messages,
                        session_clone,
                        state_map_clone,
                        halt_flag_clone,
                        timers_clone,
                        output_tx_clone.clone(),
                        event_tx_clone.clone(),
                        total_prompt_clone,
                        total_completion_clone,
                        retry_config,
                        node_timeout,
                    )
                    .await;

                    let _ = executor
                        .on_delivery_end(&EngineWorkContext::stub(&node_label))
                        .await;

                    result
                });

                handles.push(handle);
            }

            let mut routed_total = 0usize;
            let mut node_error: Option<Result<()>> = None;

            for handle in handles {
                match handle.await {
                    Ok(Ok((source_node_id, messages, _any_output))) => {
                        for msg in messages {
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
                                        next_step_ctx.enqueue(routed_env);
                                        routed_total += 1;
                                    }
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        let _ = event_tx.send(WorkflowEvent::NodeFailed {
                            node_id: "unknown".into(),
                            error: e.to_string(),
                        });
                        run_compensations(
                            &execution_log,
                            &executor_map,
                            &state_map,
                            &session,
                        )
                        .await;
                        node_error = Some(Err(e));
                        break;
                    }
                    Err(join_err) => {
                        let err = AgentError::WorkflowError(format!("节点任务 panic: {}", join_err));
                        let _ = event_tx.send(WorkflowEvent::WorkflowError {
                            error: join_err.to_string(),
                            node_id: None,
                        });
                        run_compensations(
                            &execution_log,
                            &executor_map,
                            &state_map,
                            &session,
                        )
                        .await;
                        node_error = Some(Err(err));
                        break;
                    }
                }
            }

            if let Some(Err(e)) = node_error {
                let _ = event_tx.send(WorkflowEvent::WorkflowError {
                    error: e.to_string(),
                    node_id: None,
                });
                return Err(e);
            }

            let _ = event_tx.send(WorkflowEvent::SuperStepCompleted {
                step_number: current_step_number,
                outputs_count: routed_total,
            });

            save_checkpoint(
                checkpoint_manager.as_ref(),
                &session_id,
                &graph_fingerprint,
                &state_map,
                &edge_runners_map,
                &next_step_ctx,
                current_step_number,
            )
            .await;

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

        Ok(())
    }
}

// ══ 节点执行（含重试 + 超时） ══

#[allow(clippy::too_many_arguments)]
async fn execute_node_messages(
    executor: &Arc<dyn crate::executor::IExecutor>,
    node_label: &str,
    messages: VecDeque<MessageEnvelope>,
    session: Option<Arc<dyn ISession>>,
    state_map: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    halt_flag: Arc<std::sync::atomic::AtomicBool>,
    timers: Arc<tokio::sync::Mutex<Vec<TimerEntry>>>,
    output_tx: tokio::sync::mpsc::Sender<Result<WorkflowOutput>>,
    event_tx: broadcast::Sender<WorkflowEvent>,
    total_prompt: Arc<std::sync::atomic::AtomicU32>,
    total_completion: Arc<std::sync::atomic::AtomicU32>,
    retry_config: Option<RetryConfig>,
    node_timeout: Option<Duration>,
) -> Result<(String, Vec<Arc<dyn std::any::Any + Send + Sync>>, bool)> {
    let config = retry_config.unwrap_or(RetryConfig {
        max_retries: 0,
        ..RetryConfig::default()
    });

    let mut attempt = 0u32;
    loop {
        match execute_node_messages_once(
            executor,
            node_label,
            &messages,
            session.clone(),
            state_map.clone(),
            halt_flag.clone(),
            timers.clone(),
            output_tx.clone(),
            event_tx.clone(),
            total_prompt.clone(),
            total_completion.clone(),
            node_timeout,
        )
        .await
        {
            Ok(result) => return Ok(result),
            Err(e) => {
                let err_str = e.to_string();
                if attempt < config.max_retries && config.retry_on.should_retry(&err_str) {
                    let delay = config.backoff.delay(attempt);
                    let _ = event_tx.send(WorkflowEvent::Custom {
                        key: "node_retry".into(),
                        data: serde_json::json!({
                            "node_id": node_label,
                            "attempt": attempt + 1,
                            "delay_ms": delay.as_millis(),
                            "error": err_str,
                        }),
                    });
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    attempt += 1;
                    continue;
                }

                match &config.on_exhausted {
                    ExhaustedAction::Fail => {
                        let _ = event_tx.send(WorkflowEvent::NodeFailed {
                            node_id: node_label.to_string(),
                            error: err_str,
                        });
                        return Err(e);
                    }
                    ExhaustedAction::Skip => {
                        let _ = event_tx.send(WorkflowEvent::NodeCompleted {
                            node_id: node_label.to_string(),
                            messages_produced: 0,
                            usage: None,
                        });
                        return Ok((node_label.to_string(), vec![], false));
                    }
                    ExhaustedAction::FallbackNode(fallback_id) => {
                        let fallback_msg: Arc<dyn std::any::Any + Send + Sync> =
                            Arc::new(err_str.clone());
                        return Ok((fallback_id.clone(), vec![fallback_msg], false));
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_node_messages_once(
    executor: &Arc<dyn crate::executor::IExecutor>,
    node_label: &str,
    messages: &VecDeque<MessageEnvelope>,
    session: Option<Arc<dyn ISession>>,
    state_map: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    halt_flag: Arc<std::sync::atomic::AtomicBool>,
    timers: Arc<tokio::sync::Mutex<Vec<TimerEntry>>>,
    output_tx: tokio::sync::mpsc::Sender<Result<WorkflowOutput>>,
    event_tx: broadcast::Sender<WorkflowEvent>,
    total_prompt: Arc<std::sync::atomic::AtomicU32>,
    total_completion: Arc<std::sync::atomic::AtomicU32>,
    node_timeout: Option<Duration>,
) -> Result<(String, Vec<Arc<dyn std::any::Any + Send + Sync>>, bool)> {
    let mut all_produced: Vec<Arc<dyn std::any::Any + Send + Sync>> = Vec::new();
    let mut any_output = false;

    for env in messages {
        let handle_future = async {
            let (progress_tx, mut progress_rx) =
                tokio::sync::mpsc::unbounded_channel::<NodeProgress>();

            let event_tx_progress = event_tx.clone();
            let nid = node_label.to_string();
            let prompt_counter = total_prompt.clone();
            let completion_counter = total_completion.clone();
            tokio::spawn(async move {
                while let Some(progress) = progress_rx.recv().await {
                    if let NodeProgress::UsageUpdate {
                        prompt_tokens,
                        completion_tokens,
                    } = &progress
                    {
                        prompt_counter
                            .fetch_add(*prompt_tokens, std::sync::atomic::Ordering::Relaxed);
                        completion_counter
                            .fetch_add(*completion_tokens, std::sync::atomic::Ordering::Relaxed);
                    }
                    let chunk = node_progress_to_chunk(progress);
                    let _ = event_tx_progress.send(WorkflowEvent::NodeStreaming {
                        node_id: nid.clone(),
                        chunk,
                    });
                }
            });

            let queued_msgs: Arc<
                parking_lot::Mutex<Vec<Arc<dyn std::any::Any + Send + Sync>>>,
            > = Arc::new(parking_lot::Mutex::new(Vec::new()));

            let work_ctx = EngineWorkContext {
                node_id: node_label.to_string(),
                session: session.clone(),
                state_map: state_map.clone(),
                queued_messages: queued_msgs.clone(),
                output_tx: output_tx.clone(),
                event_tx: event_tx.clone(),
                halt_flag: halt_flag.clone(),
                timers: timers.clone(),
            };

            let result = executor
                .handle(env.content.clone(), &work_ctx, progress_tx)
                .await?;

            Ok::<_, AgentError>((result, queued_msgs))
        };

        let (result, queued_msgs) = match node_timeout {
            Some(timeout) => match tokio::time::timeout(timeout, handle_future).await {
                Ok(inner) => inner?,
                Err(_) => {
                    return Err(AgentError::WorkflowError(format!(
                        "节点 {} 执行超时 ({:?})",
                        node_label, timeout
                    )));
                }
            },
            None => handle_future.await?,
        };

        match result {
            HandlerResult::Messages(mut msgs) => {
                let mut queued = queued_msgs.lock();
                msgs.extend(queued.drain(..).collect::<Vec<_>>());
                all_produced.extend(msgs);
            }
            HandlerResult::Output(output) => {
                let _ = output_tx
                    .send(Ok(WorkflowOutput {
                        content: output,
                        source_node_id: node_label.to_string(),
                    }))
                    .await;
                any_output = true;
            }
            HandlerResult::None => {
                let mut queued = queued_msgs.lock();
                all_produced.extend(queued.drain(..).collect::<Vec<_>>());
            }
        }
    }

    let msg_count = all_produced.len();
    let _ = event_tx.send(WorkflowEvent::NodeCompleted {
        node_id: node_label.to_string(),
        messages_produced: msg_count,
        usage: None,
    });

    Ok((node_label.to_string(), all_produced, any_output))
}

// ══ 定时器 ══

async fn fire_due_timers(
    timers: &Arc<tokio::sync::Mutex<Vec<TimerEntry>>>,
    executor_map: &HashMap<String, Arc<dyn crate::executor::IExecutor>>,
    event_tx: &broadcast::Sender<WorkflowEvent>,
    step_ctx: &mut StepContext,
) {
    let now = Instant::now();
    let mut timer_guard = timers.lock().await;
    let (due, pending): (Vec<TimerEntry>, Vec<TimerEntry>) =
        timer_guard.drain(..).partition(|t| t.fires_at <= now);
    *timer_guard = pending;

    for entry in due {
        let _ = event_tx.send(WorkflowEvent::TimerFired {
            node_id: entry.node_id.clone(),
            timer_name: entry.timer_name.clone(),
        });

        if let Some(executor) = executor_map.get(&entry.node_id) {
            let stub = EngineWorkContext::stub(&entry.node_id);
            let _ = executor.on_timer(&entry.timer_name, &stub).await;
        }

        let env = MessageEnvelope::new(
            &entry.node_id,
            Arc::new(entry.timer_name.clone()) as Arc<dyn std::any::Any + Send + Sync>,
            crate::executor::TypeTag::new("timer"),
        )
        .with_target(&entry.node_id);
        step_ctx.enqueue(env);
    }
}

// ══ 补偿 ══

async fn run_compensations(
    execution_log: &Arc<tokio::sync::Mutex<Vec<String>>>,
    executor_map: &HashMap<String, Arc<dyn crate::executor::IExecutor>>,
    state_map: &Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    session: &Option<Arc<dyn ISession>>,
) {
    let log = execution_log.lock().await.clone();
    for node_id in log.iter().rev() {
        if let Some(executor) = executor_map.get(node_id) {
            let ctx = EngineWorkContext {
                node_id: node_id.clone(),
                session: session.clone(),
                state_map: state_map.clone(),
                queued_messages: Arc::new(parking_lot::Mutex::new(Vec::new())),
                output_tx: {
                    let (tx, _) = tokio::sync::mpsc::channel(1);
                    tx
                },
                event_tx: {
                    let (tx, _) = broadcast::channel(1);
                    tx
                },
                halt_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                timers: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            };
            let _ = executor.compensate(&ctx).await;
        }
    }
}

// ══ Checkpoint 辅助 ══

async fn save_checkpoint(
    checkpoint_manager: Option<&Arc<CheckpointManager>>,
    session_id: &str,
    graph_fingerprint: &str,
    state_map: &Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    edge_runners_map: &HashMap<String, Arc<dyn IEdgeRunner>>,
    step_ctx: &StepContext,
    step_number: i32,
) {
    if let Some(cp) = checkpoint_manager {
        let current_state: HashMap<String, serde_json::Value> = state_map.lock().await.clone();
        let scope_state: HashMap<ScopeKey, serde_json::Value> = current_state
            .into_iter()
            .map(|(k, v)| (ScopeKey::private(&k), v))
            .collect();

        let mut edge_states: HashMap<String, serde_json::Value> = HashMap::new();
        for (key, runner) in edge_runners_map {
            for (k, v) in runner.checkpoint_state() {
                edge_states.insert(format!("{}:{}", key, k), v);
            }
        }

        let pending = step_ctx.serialize_pending();

        if let Err(e) = cp
            .commit(
                session_id,
                graph_fingerprint,
                scope_state,
                edge_states,
                pending,
                step_number,
            )
            .await
        {
            tracing::warn!(error = %e, "Failed to commit checkpoint");
        }
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

// ══ EngineWorkContext ══

struct EngineWorkContext {
    node_id: String,
    session: Option<Arc<dyn ISession>>,
    state_map: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    queued_messages: Arc<parking_lot::Mutex<Vec<Arc<dyn std::any::Any + Send + Sync>>>>,
    output_tx: tokio::sync::mpsc::Sender<Result<WorkflowOutput>>,
    event_tx: broadcast::Sender<WorkflowEvent>,
    halt_flag: Arc<std::sync::atomic::AtomicBool>,
    timers: Arc<tokio::sync::Mutex<Vec<TimerEntry>>>,
}

impl EngineWorkContext {
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
            timers: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl crate::engine::IWorkflowContext for EngineWorkContext {
    async fn send_message(&self, envelope: MessageEnvelope) -> Result<()> {
        self.queued_messages.lock().push(envelope.content);
        Ok(())
    }

    async fn yield_output(&self, output: Arc<dyn std::any::Any + Send + Sync>) -> Result<()> {
        let _ = self
            .output_tx
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
        self.halt_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
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

    async fn schedule_timer(&self, name: &str, delay: Duration) -> Result<()> {
        let mut timers = self.timers.lock().await;
        timers.push(TimerEntry {
            node_id: self.node_id.clone(),
            timer_name: name.to_string(),
            fires_at: Instant::now() + delay,
        });
        Ok(())
    }

    async fn variable_names(&self) -> Vec<String> {
        let state = self.state_map.lock().await;
        state.keys().cloned().collect()
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
    use crate::checkpoint::store::InMemoryCheckpointStore;
    use crate::executor::FunctionExecutor;
    use rust_agent_core::AgentSession;

    #[tokio::test]
    async fn test_checkpoint_create_initial_and_commit_timing() {
        let graph = WorkflowBuilder::new()
            .add_node(
                "entry",
                Arc::new(FunctionExecutor::new("entry", |msg: String| {
                    vec![format!("processed: {}", msg)]
                })),
            )
            .set_start("entry")
            .with_output_from("entry")
            .build()
            .expect("should build graph");

        let store = Arc::new(InMemoryCheckpointStore::new());
        let cp_manager = Arc::new(CheckpointManager::with_default_config(store));

        let engine = WorkflowEngine::new(graph).with_checkpoint_manager(cp_manager);

        let session: Arc<dyn ISession> = Arc::new(AgentSession::with_id("test-session"));

        let (mut events, _outputs) = engine
            .run(Arc::new("hello".to_string()), Some(session))
            .await
            .expect("should start engine");

        let mut event_count = 0;
        let timeout = tokio::time::sleep(Duration::from_secs(2));
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

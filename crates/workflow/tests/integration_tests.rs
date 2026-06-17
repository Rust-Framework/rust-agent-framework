//! 集成测试 — 覆盖 WorkflowGraph 构建/验证、FunctionExecutor、EdgeRunner、Engine

use std::sync::Arc;

use rust_agent_workflow::{Edge, FunctionExecutor, HandlerResult, IExecutor, WorkflowBuilder};

// ═══════════════════════════════════════════════════
// WorkflowBuilder + WorkflowGraph 构建和验证测试
// ═══════════════════════════════════════════════════

#[test]
fn test_builder_basic_graph() {
    let graph = WorkflowBuilder::new()
        .add_node("entry", Arc::new(FunctionExecutor::new("entry", |msg: String| {
            vec![format!("processed: {}", msg)]
        })))
        .set_start("entry")
        .with_output_from("entry")
        .build()
        .expect("should build simple graph");

    assert_eq!(graph.start_node_id(), "entry");
    assert!(graph.output_node_ids().contains("entry"));
    assert!(graph.get_node("entry").is_some());
}

#[test]
fn test_builder_missing_start_should_fail() {
    let result = WorkflowBuilder::new()
        .add_node("node_a", Arc::new(FunctionExecutor::new("node_a", |_: String| vec!["ok".to_string()])))
        .build();
    assert!(result.is_err(), "Should fail without start node");
}

#[test]
fn test_builder_missing_node_should_fail() {
    let result = WorkflowBuilder::new()
        .add_node("node_a", Arc::new(FunctionExecutor::new("node_a", |_: String| vec!["ok".to_string()])))
        .set_start("node_a")
        .add_edge("node_a", "non_existent")
        .build();
    assert!(result.is_err(), "Should fail when edge references non-existent node");
}

#[test]
fn test_builder_missing_output_node_should_fail() {
    let result = WorkflowBuilder::new()
        .add_node("node_a", Arc::new(FunctionExecutor::new("node_a", |_: String| vec!["ok".to_string()])))
        .set_start("node_a")
        .with_output_from("non_existent")
        .build();
    assert!(result.is_err(), "Should fail when output node doesn't exist");
}

#[test]
fn test_builder_multi_node_graph() {
    let graph = WorkflowBuilder::new()
        .add_node("entry", Arc::new(FunctionExecutor::new("entry", |msg: String| {
            vec![format!("step1: {}", msg)]
        })))
        .add_node("processor", Arc::new(FunctionExecutor::new("processor", |msg: String| {
            vec![format!("step2: {}", msg)]
        })))
        .set_start("entry")
        .add_edge("entry", "processor")
        .with_output_from("processor")
        .build()
        .expect("should build multi-node graph");

    assert_eq!(graph.nodes().len(), 2);
    assert_eq!(graph.start_node_id(), "entry");
    assert!(graph.output_node_ids().contains("processor"));
}

#[test]
fn test_builder_fan_out_edge() {
    let graph = WorkflowBuilder::new()
        .add_node("entry", Arc::new(FunctionExecutor::new("entry", |msg: String| {
            vec![msg.clone(), format!("copy: {}", msg)]
        })))
        .add_node("sink_a", Arc::new(FunctionExecutor::new("sink_a", |_: String| vec!["ok".to_string()])))
        .add_node("sink_b", Arc::new(FunctionExecutor::new("sink_b", |_: String| vec!["ok".to_string()])))
        .set_start("entry")
        .add_fan_out_edge("entry", vec!["sink_a", "sink_b"])
        .build()
        .expect("should build fan-out graph");

    let edges = graph.get_edges_from("entry").expect("entry should have edges");
    assert_eq!(edges.len(), 1);
    let edge = edges.iter().next().unwrap();
    assert!(matches!(edge, Edge::FanOut(_)));
}

#[test]
fn test_builder_fan_in_edge() {
    let graph = WorkflowBuilder::new()
        .add_node("source_a", Arc::new(FunctionExecutor::new("source_a", |_: String| {
            vec!["from_a".to_string()]
        })))
        .add_node("source_b", Arc::new(FunctionExecutor::new("source_b", |_: String| {
            vec!["from_b".to_string()]
        })))
        .add_node("sink", Arc::new(FunctionExecutor::new("sink", |_: String| vec!["ok".to_string()])))
        .set_start("source_a")
        .add_fan_in_edge(vec!["source_a", "source_b"], "sink")
        .build()
        .expect("should build fan-in graph");

    let edges_a = graph.get_edges_from("source_a").expect("source_a should have edges");
    let edges_b = graph.get_edges_from("source_b").expect("source_b should have edges");
    assert_eq!(edges_a.len(), 1);
    assert_eq!(edges_b.len(), 1);
}

// ═══════════════════════════════════════════════════
// 环检测测试
// ═══════════════════════════════════════════════════

fn make_fn_exec(id: &str) -> impl IExecutor {
    let id_owned = id.to_string();
    FunctionExecutor::new(id, move |_: String| vec![id_owned.clone()])
}

#[test]
fn test_cycle_detection_simple_loop() {
    let result = WorkflowBuilder::new()
        .add_node("a", Arc::new(make_fn_exec("a")))
        .add_node("b", Arc::new(make_fn_exec("b")))
        .set_start("a")
        .add_edge("a", "b")
        .add_edge("b", "a")
        .build();
    assert!(result.is_err(), "Should detect simple cycle");
}

#[test]
fn test_cycle_detection_self_loop() {
    let result = WorkflowBuilder::new()
        .add_node("a", Arc::new(make_fn_exec("a")))
        .set_start("a")
        .add_edge("a", "a")
        .build();
    assert!(result.is_err(), "Should detect self-loop");
}

#[test]
fn test_cycle_detection_three_node_cycle() {
    let result = WorkflowBuilder::new()
        .add_node("a", Arc::new(make_fn_exec("a")))
        .add_node("b", Arc::new(make_fn_exec("b")))
        .add_node("c", Arc::new(make_fn_exec("c")))
        .set_start("a")
        .add_edge("a", "b")
        .add_edge("b", "c")
        .add_edge("c", "a")
        .build();
    assert!(result.is_err(), "Should detect three-node cycle");
}

#[test]
fn test_dag_no_cycle_should_pass() {
    let graph = WorkflowBuilder::new()
        .add_node("a", Arc::new(make_fn_exec("a")))
        .add_node("b", Arc::new(make_fn_exec("b")))
        .add_node("c", Arc::new(make_fn_exec("c")))
        .set_start("a")
        .add_edge("a", "b")
        .add_edge("b", "c")
        .build();
    assert!(graph.is_ok(), "DAG should pass cycle detection");
}

// ═══════════════════════════════════════════════════
// FunctionExecutor 测试
// ═══════════════════════════════════════════════════

#[test]
fn test_function_executor_id() {
    let executor = FunctionExecutor::new("double", |msg: String| -> Vec<String> {
        vec![format!("{}{}", msg, msg)]
    });
    assert_eq!(executor.id(), "double");
}

#[test]
fn test_function_executor_type_info() {
    let executor = FunctionExecutor::new("prefixer", |msg: String| -> Vec<String> {
        vec![format!("prefixed_{}", msg)]
    });
    let types = executor.accepted_types();
    assert_eq!(types.len(), 1);
    assert!(types[0].type_name.contains("String"));
}

// ═══════════════════════════════════════════════════
// HandlerResult 转换测试
// ═══════════════════════════════════════════════════

#[test]
fn test_handler_result_from_unit() {
    let result: HandlerResult = ().into();
    assert!(matches!(result, HandlerResult::None));
}

#[test]
fn test_handler_result_from_vec_string() {
    let result: HandlerResult = vec!["hello".to_string(), "world".to_string()].into();
    match result {
        HandlerResult::Messages(msgs) => assert_eq!(msgs.len(), 2),
        _ => panic!("Expected Messages"),
    }
}

// ═══════════════════════════════════════════════════
// Edge 结构测试
// ═══════════════════════════════════════════════════

#[test]
fn test_edge_id_equality() {
    use rust_agent_workflow::graph::edge::{DirectEdgeData, EdgeId};

    let id1 = Edge::Direct(DirectEdgeData {
        edge_id: EdgeId::new("e1"),
        source_id: "a".into(),
        sink_id: "b".into(),
        label: None,
        condition: None,
    });
    let id2 = Edge::Direct(DirectEdgeData {
        edge_id: EdgeId::new("e1"),
        source_id: "c".into(),
        sink_id: "d".into(),
        label: None,
        condition: None,
    });
    assert_eq!(id1.edge_id(), id2.edge_id());
}

#[test]
fn test_edge_source_sink_ids() {
    use rust_agent_workflow::graph::edge::{DirectEdgeData, EdgeId};

    let edge = Edge::Direct(DirectEdgeData {
        edge_id: EdgeId::new("test_edge"),
        source_id: "src".into(),
        sink_id: "dst".into(),
        label: None,
        condition: None,
    });
    assert_eq!(edge.source_ids(), vec!["src"]);
    assert_eq!(edge.sink_ids(), vec!["dst"]);
}

// ═══════════════════════════════════════════════════
// WorkflowEngine 集成测试
// ═══════════════════════════════════════════════════

#[tokio::test]
async fn test_engine_single_node_workflow() {
    use futures_util::StreamExt;
    use rust_agent_workflow::{WorkflowEngine, WorkflowEvent};

    let graph = WorkflowBuilder::new()
        .add_node("entry", Arc::new(FunctionExecutor::new("entry", |msg: String| {
            vec![format!("processed: {}", msg)]
        })))
        .set_start("entry")
        .with_output_from("entry")
        .build()
        .expect("should build graph");

    let engine = WorkflowEngine::new(graph);
    let (mut events, _outputs) = engine
        .run(Arc::new("test_input".to_string()), None)
        .await
        .expect("Engine should start");

    let mut event_count = 0;
    let mut saw_started = false;
    let mut saw_completed = false;

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(event) = events.next() => {
                event_count += 1;
                match event {
                    WorkflowEvent::WorkflowStarted { .. } => saw_started = true,
                    WorkflowEvent::WorkflowCompleted { .. } => saw_completed = true,
                    _ => {}
                }
            }
            _ = &mut timeout => break,
        }
    }

    assert!(event_count > 0, "Should produce workflow events");
    assert!(saw_started, "Should emit WorkflowStarted");
    assert!(saw_completed, "Should emit WorkflowCompleted");
}

#[tokio::test]
async fn test_engine_two_node_sequential() {
    use futures_util::StreamExt;
    use rust_agent_workflow::WorkflowEngine;

    let graph = WorkflowBuilder::new()
        .add_node("first", Arc::new(FunctionExecutor::new("first", |msg: String| {
            vec![format!("first: {}", msg)]
        })))
        .add_node("second", Arc::new(FunctionExecutor::new("second", |msg: String| {
            vec![format!("second: {}", msg)]
        })))
        .set_start("first")
        .add_edge("first", "second")
        .with_output_from("second")
        .build()
        .expect("should build two-node graph");

    let engine = WorkflowEngine::new(graph);
    let (mut events, _outputs) = engine
        .run(Arc::new("hello".to_string()), None)
        .await
        .expect("should start engine");

    let mut event_count = 0;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(_) = events.next() => { event_count += 1; }
            _ = &mut timeout => break,
        }
    }
    assert!(event_count > 0, "Two-node workflow should produce events");
}

#[tokio::test]
async fn test_engine_fan_out_execution() {
    use futures_util::StreamExt;
    use rust_agent_workflow::WorkflowEngine;

    let graph = WorkflowBuilder::new()
        .add_node("entry", Arc::new(FunctionExecutor::new("entry", |msg: String| {
            vec![msg, "fan_out".to_string()]
        })))
        .add_node("sink_a", Arc::new(FunctionExecutor::new("sink_a", |_: String| vec!["ok".to_string()])))
        .add_node("sink_b", Arc::new(FunctionExecutor::new("sink_b", |_: String| vec!["ok".to_string()])))
        .set_start("entry")
        .add_fan_out_edge("entry", vec!["sink_a", "sink_b"])
        .build()
        .expect("should build fan-out graph");

    let engine = WorkflowEngine::new(graph);
    let (mut events, _outputs) = engine
        .run(Arc::new("start".to_string()), None)
        .await
        .expect("should start engine");

    let mut event_count = 0;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(_) = events.next() => { event_count += 1; }
            _ = &mut timeout => break,
        }
    }
    assert!(event_count > 0, "Fan-out workflow should produce events");
}

#[tokio::test]
async fn test_engine_checkpoint_integration() {
    use futures_util::StreamExt;
    use rust_agent_core::{AgentSession, ISession};
    use rust_agent_workflow::{
        CheckpointManager, InMemoryCheckpointStore, WorkflowEngine,
    };

    let graph = WorkflowBuilder::new()
        .add_node("node", Arc::new(FunctionExecutor::new("node", |msg: String| {
            vec![format!("echo: {}", msg)]
        })))
        .set_start("node")
        .with_output_from("node")
        .build()
        .expect("should build graph");

    let store = Arc::new(InMemoryCheckpointStore::new());
    let cp_manager = Arc::new(CheckpointManager::with_default_config(store));
    let engine = WorkflowEngine::new(graph).with_checkpoint_manager(cp_manager.clone());
    let session: Arc<dyn ISession> = Arc::new(AgentSession::with_id("cp-test-int"));

    let (mut events, _outputs) = engine
        .run(Arc::new("test".to_string()), Some(session.clone()))
        .await
        .expect("should start engine");

    let mut event_count = 0;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(_) = events.next() => { event_count += 1; }
            _ = &mut timeout => break,
        }
    }

    assert!(event_count > 0, "Should produce events with checkpoint enabled");

    let info = cp_manager
        .get_latest_info(session.session_id())
        .await
        .expect("should query checkpoint");
    assert!(info.is_some(), "Should have saved at least one checkpoint");
}

// ═══════════════════════════════════════════════════
// 流程引擎化特性测试
// ═══════════════════════════════════════════════════

#[tokio::test]
async fn test_flow_variables() {
    use futures_util::StreamExt;
    use rust_agent_workflow::{WorkflowEngine, WorkflowEvent};

    let graph = WorkflowBuilder::new()
        .add_node(
            "entry",
            Arc::new(FunctionExecutor::new("entry", |msg: String| {
                vec![format!("got: {}", msg)]
            })),
        )
        .set_start("entry")
        .with_output_from("entry")
        .build()
        .expect("build graph");

    let engine = WorkflowEngine::new(graph);
    let (mut events, _) = engine
        .run(Arc::new("input".to_string()), None)
        .await
        .expect("start");

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    let mut completed = false;
    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                if matches!(ev, WorkflowEvent::WorkflowCompleted { .. }) {
                    completed = true;
                }
            }
            _ = &mut timeout => break,
        }
    }
    assert!(completed);
}

#[tokio::test]
async fn test_node_retry_on_failure() {
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use rust_agent_workflow::{
        ExhaustedAction, HandlerResult, IExecutor, NodeProgress, RetryBackoff, RetryCondition,
        RetryConfig, WorkflowEngine, WorkflowEvent,
    };
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    struct FlakyExecutor {
        id: String,
        attempts: Arc<AtomicU32>,
    }

    #[async_trait]
    impl IExecutor for FlakyExecutor {
        fn id(&self) -> &str {
            &self.id
        }

        async fn handle(
            &self,
            _message: Arc<dyn std::any::Any + Send + Sync>,
            _ctx: &dyn rust_agent_workflow::IWorkflowContext,
            _progress: tokio::sync::mpsc::UnboundedSender<NodeProgress>,
        ) -> rust_agent_core::Result<HandlerResult> {
            let n = self.attempts.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                return Err(rust_agent_core::AgentError::WorkflowError(
                    "transient".into(),
                ));
            }
            Ok(HandlerResult::None)
        }
    }

    let attempt_count = Arc::new(AtomicU32::new(0));

    let graph = WorkflowBuilder::new()
        .add_node(
            "flaky",
            Arc::new(FlakyExecutor {
                id: "flaky".into(),
                attempts: attempt_count.clone(),
            }),
        )
        .with_retry(RetryConfig {
            max_retries: 3,
            backoff: RetryBackoff::None,
            retry_on: RetryCondition::AllErrors,
            on_exhausted: ExhaustedAction::Skip,
        })
        .set_start("flaky")
        .with_output_from("flaky")
        .build()
        .expect("build");

    let engine = WorkflowEngine::new(graph);
    let (mut events, _) = engine
        .run(Arc::new("test".to_string()), None)
        .await
        .expect("start");

    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);
    let mut completed = false;
    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                if matches!(ev, WorkflowEvent::WorkflowCompleted { .. }) {
                    completed = true;
                }
            }
            _ = &mut timeout => break,
        }
    }
    assert!(completed);
    assert!(attempt_count.load(Ordering::SeqCst) >= 2);
}

#[tokio::test]
async fn test_human_task_halt_and_resume() {
    use futures_util::StreamExt;
    use rust_agent_workflow::{
        HumanTaskExecutor, ResumeCommand, WorkflowEvent, WorkflowRuntime,
    };

    let graph = WorkflowBuilder::new()
        .add_node(
            "approval",
            Arc::new(HumanTaskExecutor::new(
                "approval",
                Arc::new(|_ctx| serde_json::json!({"form": "approve?"})),
            )),
        )
        .add_node(
            "downstream",
            Arc::new(FunctionExecutor::new("downstream", |val: serde_json::Value| {
                vec![val]
            })),
        )
        .set_start("approval")
        .add_edge("approval", "downstream")
        .with_output_from("downstream")
        .build()
        .expect("build");

    let runtime = WorkflowRuntime::start(
        graph,
        Arc::new("request".to_string()),
        None,
    )
    .await
    .expect("start runtime");

    let mut events = runtime.events().await.expect("events");
    let mut saw_halted = false;

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(5));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                if matches!(ev, WorkflowEvent::WorkflowHalted { .. }) {
                    saw_halted = true;
                    runtime
                        .resume(ResumeCommand::InjectMessage {
                            target_node_id: "approval".into(),
                            message: Arc::new(serde_json::json!({"approved": true})),
                        })
                        .expect("resume");
                }
                if matches!(ev, WorkflowEvent::WorkflowCompleted { .. }) {
                    break;
                }
            }
            _ = &mut timeout => break,
        }
    }

    assert!(saw_halted, "Should halt for human task");
    let _ = runtime.wait().await;
}

#[tokio::test]
async fn test_workflow_config_max_parallel() {
    use futures_util::StreamExt;
    use rust_agent_workflow::{WorkflowConfig, WorkflowEngine};

    let graph = WorkflowBuilder::new()
        .add_node("a", Arc::new(FunctionExecutor::new("a", |_: String| vec!["ok".to_string()])))
        .add_node("b", Arc::new(FunctionExecutor::new("b", |_: String| vec!["ok".to_string()])))
        .set_start("a")
        .add_fan_out_edge("a", vec!["b", "b"])
        .build()
        .expect("build");

    let config = WorkflowConfig::new().with_max_parallel(1);
    let engine = WorkflowEngine::new(graph).with_config(config);
    let (mut events, _) = engine
        .run(Arc::new("x".to_string()), None)
        .await
        .expect("start");

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    let mut count = 0;
    loop {
        tokio::select! {
            Some(_) = events.next() => { count += 1; }
            _ = &mut timeout => break,
        }
    }
    assert!(count > 0);
}

#[tokio::test]
async fn test_compensable_executor_on_failure() {
    use async_trait::async_trait;
    use futures_util::StreamExt;
    use rust_agent_workflow::{HandlerResult, IExecutor, NodeProgress, WorkflowEngine};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FailingExecutor {
        compensated: Arc<AtomicBool>,
    }

    #[async_trait]
    impl IExecutor for FailingExecutor {
        fn id(&self) -> &str {
            "fail"
        }

        async fn handle(
            &self,
            _message: Arc<dyn std::any::Any + Send + Sync>,
            _ctx: &dyn rust_agent_workflow::IWorkflowContext,
            _progress: tokio::sync::mpsc::UnboundedSender<NodeProgress>,
        ) -> rust_agent_core::Result<HandlerResult> {
            Err(rust_agent_core::AgentError::WorkflowError("boom".into()))
        }

        async fn compensate(&self, _ctx: &dyn rust_agent_workflow::IWorkflowContext) -> rust_agent_core::Result<()> {
            self.compensated.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    let compensated = Arc::new(AtomicBool::new(false));

    let graph = WorkflowBuilder::new()
        .add_node(
            "fail",
            Arc::new(FailingExecutor {
                compensated: compensated.clone(),
            }),
        )
        .set_start("fail")
        .build()
        .expect("build");

    let engine = WorkflowEngine::new(graph);
    let (mut events, _) = engine
        .run(Arc::new("x".to_string()), None)
        .await
        .expect("start");

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(_) = events.next() => {}
            _ = &mut timeout => break,
        }
    }
    assert!(compensated.load(Ordering::SeqCst));
}

//! 集成测试 — 覆盖 WorkflowGraph 构建/验证、FunctionExecutor、EdgeRunner、Engine

use std::sync::Arc;

use rust_agent_workflow::{
    Edge, FunctionExecutor, HandlerResult, IExecutor, WorkflowBuilder,
};

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
    assert!(graph.get_node("entry").is_some());
    assert!(graph.get_node("processor").is_some());
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
    let sinks: Vec<&str> = edge.sink_ids();
    assert!(sinks.contains(&"sink_a"));
    assert!(sinks.contains(&"sink_b"));
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
        .add_edge("b", "a") // 回环!
        .build();

    assert!(result.is_err(), "Should detect simple cycle");
}

#[test]
fn test_cycle_detection_self_loop() {
    let result = WorkflowBuilder::new()
        .add_node("a", Arc::new(make_fn_exec("a")))
        .set_start("a")
        .add_edge("a", "a") // 自环!
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
        .add_edge("c", "a") // 三节点回环!
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
// WorkflowEngine 基础集成测试
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
    let initial: Box<dyn std::any::Any + Send + Sync> = Box::new("test_input".to_string());

    let result = engine.run(initial, None).await;
    assert!(result.is_ok(), "Engine should start successfully");

    let (mut events, _outputs) = result.unwrap();

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
        .run(Box::new("hello".to_string()), None)
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
        .run(Box::new("start".to_string()), None)
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
        .run(Box::new("test".to_string()), Some(session.clone()))
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

    // 验证检查点已保存
    let info = cp_manager
        .get_latest_info(session.session_id())
        .await
        .expect("should query checkpoint");
    assert!(info.is_some(), "Should have saved at least one checkpoint");
}

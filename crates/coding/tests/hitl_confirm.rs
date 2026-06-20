//! 测试 HumanTaskExecutor 暂停与恢复 — 验证 HITL 机制。

use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_workflow::{
    FunctionExecutor, HumanTaskExecutor, ResumeCommand, WorkflowBuilder, WorkflowEvent,
    WorkflowRuntime,
};

#[tokio::test]
async fn test_human_task_halt_and_resume() {
    // 构建最小图: entry → approval (HITL) → output
    let graph = WorkflowBuilder::new()
        .add_node(
            "entry",
            Arc::new(FunctionExecutor::new("entry", |msg: String| {
                vec![format!("需求: {}", msg)]
            })),
        )
        .add_node(
            "approval",
            Arc::new(HumanTaskExecutor::new(
                "approval",
                Arc::new(|_ctx| {
                    serde_json::json!({
                        "task": "需求确认",
                        "instruction": "请确认需求"
                    })
                }),
            )),
        )
        .add_node(
            "output",
            Arc::new(FunctionExecutor::new("output", |msg: serde_json::Value| {
                vec![msg]
            })),
        )
        .set_start("entry")
        .add_edge("entry", "approval")
        .add_edge("approval", "output")
        .with_output_from("output")
        .build()
        .expect("build graph");

    // 启动 runtime
    let runtime = WorkflowRuntime::start(graph, Arc::new("实现 TODO 应用".to_string()), None)
        .await
        .expect("start runtime");

    let mut events = runtime.events().await.expect("events");
    let mut saw_halted = false;
    let mut saw_completed = false;
    let mut last_node_id = String::new();
    let mut resumed = false;

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(30));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                match ev {
                    WorkflowEvent::NodeInvoking { node_id, .. } => {
                        last_node_id = node_id;
                    }
                    WorkflowEvent::WorkflowHalted { .. } => {
                        saw_halted = true;
                        if !resumed {
                            resumed = true;
                            // 工作流已完全暂停，现在恢复
                            runtime.resume(ResumeCommand::InjectMessage {
                                target_node_id: last_node_id.clone(),
                                message: Arc::new("确认".to_string()),
                            }).expect("resume");
                        }
                    }
                    WorkflowEvent::WorkflowCompleted { .. } => {
                        saw_completed = true;
                        break;
                    }
                    _ => {}
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    assert!(saw_halted, "应该触发暂停等待人工确认");
    assert!(saw_completed, "恢复后应该完成");

    let _ = runtime.wait().await;
}

#[tokio::test]
async fn test_human_task_yields_payload() {
    // 验证 HumanTaskExecutor 通过 Custom 事件输出 payload
    let graph = WorkflowBuilder::new()
        .add_node(
            "approval",
            Arc::new(HumanTaskExecutor::new(
                "approval",
                Arc::new(|_ctx| {
                    serde_json::json!({
                        "task": "测试任务",
                        "instruction": "测试指令"
                    })
                }),
            )),
        )
        .add_node(
            "output",
            Arc::new(FunctionExecutor::new("output", |msg: serde_json::Value| {
                vec![msg]
            })),
        )
        .set_start("approval")
        .add_edge("approval", "output")
        .with_output_from("output")
        .build()
        .expect("build graph");

    let runtime = WorkflowRuntime::start(graph, Arc::new("start".to_string()), None)
        .await
        .expect("start runtime");

    let mut events = runtime.events().await.expect("events");
    let mut payload_received = None;
    let mut last_node_id = String::new();

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                match ev {
                    WorkflowEvent::NodeInvoking { node_id, .. } => {
                        last_node_id = node_id;
                    }
                    WorkflowEvent::Custom { key, data } if key == "halt_payload" => {
                        payload_received = Some(data);
                    }
                    WorkflowEvent::WorkflowHalted { .. } => {
                        // 工作流已完全暂停，现在恢复
                        runtime.resume(ResumeCommand::InjectMessage {
                            target_node_id: last_node_id.clone(),
                            message: Arc::new("ok".to_string()),
                        }).expect("resume");
                    }
                    WorkflowEvent::WorkflowCompleted { .. } => break,
                    _ => {}
                }
            }
            _ = &mut timeout => break,
        }
    }

    let payload = payload_received.expect("应该收到 halt_payload");
    assert_eq!(payload["task"].as_str(), Some("测试任务"));
    assert_eq!(payload["instruction"].as_str(), Some("测试指令"));
}

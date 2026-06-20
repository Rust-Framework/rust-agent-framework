//! 测试反馈循环网关 — 验证 review_gateway 的审查通过/未通过路由。

use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_coding::executors::review_gateway;
use rust_agent_coding::state::{state_keys, ReviewVerdict};
use rust_agent_core::ChatMessage;
use rust_agent_workflow::{
    ContextFunctionExecutor, FunctionExecutor, HandlerResult, IExecutor, WorkflowBuilder,
    WorkflowEvent, WorkflowRuntime,
};

/// 辅助：创建一个将审查结论写入状态的节点
fn verdict_persister(node_id: &str, verdict_json: &str) -> Arc<dyn IExecutor> {
    let node_id = node_id.to_string();
    let verdict = verdict_json.to_string();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        move |msg, ctx, _progress| {
            let verdict = verdict.clone();
            async move {
                ctx.write_state(
                    state_keys::REVIEW_FEEDBACK,
                    serde_json::Value::String(verdict),
                )
                .await
                .unwrap();
                // 透传消息（包含审查结论）
                Ok(HandlerResult::Messages(vec![msg]))
            }
        },
    ))
}

#[tokio::test]
async fn test_review_gateway_passes_on_approved() {
    // 审查通过的场景：review_gateway 应产生输出，不沿回边路由
    let passed_verdict =
        r#"{"passed": true, "discrepancies": [], "root_cause": "", "fix_suggestions": []}"#;

    let graph = WorkflowBuilder::new()
        .add_node(
            "entry",
            Arc::new(FunctionExecutor::new("entry", move |_msg: String| {
                vec![ChatMessage::assistant(passed_verdict)]
            })),
        )
        .add_node("persister", verdict_persister("persister", passed_verdict))
        .add_node("gateway", review_gateway("gateway"))
        .add_node(
            "loop_target",
            Arc::new(FunctionExecutor::new("loop_target", |msg: ChatMessage| {
                vec![msg]
            })),
        )
        .set_start("entry")
        .add_edge("entry", "persister")
        .add_edge("persister", "gateway")
        .add_loopback_edge("gateway", "loop_target")
        .build()
        .expect("build graph");

    let runtime = WorkflowRuntime::start(graph, Arc::new("start".to_string()), None)
        .await
        .expect("start runtime");

    let mut events = runtime.events().await.expect("events");
    let mut completed = false;
    let mut loop_target_invoked = false;

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                match ev {
                    WorkflowEvent::NodeInvoking { node_id, .. } => {
                        if node_id == "loop_target" {
                            loop_target_invoked = true;
                        }
                    }
                    WorkflowEvent::WorkflowCompleted { .. } => {
                        completed = true;
                        break;
                    }
                    _ => {}
                }
            }
            _ = &mut timeout => break,
        }
    }

    assert!(completed, "工作流应该完成");
    assert!(!loop_target_invoked, "审查通过时不应触发 loop_target");

    // 验证产生了工作流输出
    if let Some(mut outputs) = runtime.outputs().await {
        let mut has_output = false;
        while let Some(Ok(_output)) = outputs.next().await {
            has_output = true;
        }
        assert!(has_output, "审查通过时应产生工作流输出");
    }

    let _ = runtime.wait().await;
}

#[tokio::test]
async fn test_review_gateway_loops_on_rejected() {
    // 审查未通过的场景：review_gateway 应沿回边路由到 loop_target
    let failed_verdict = r#"{"passed": false, "discrepancies": ["缺失测试"], "root_cause": "实现", "fix_suggestions": ["补充测试"]}"#;

    let graph = WorkflowBuilder::new()
        .add_node(
            "entry",
            Arc::new(FunctionExecutor::new("entry", move |_msg: String| {
                vec![ChatMessage::assistant(failed_verdict)]
            })),
        )
        .add_node("persister", verdict_persister("persister", failed_verdict))
        .add_node("gateway", review_gateway("gateway"))
        .add_node(
            "loop_target",
            Arc::new(FunctionExecutor::new("loop_target", |msg: ChatMessage| {
                vec![msg]
            })),
        )
        .set_start("entry")
        .add_edge("entry", "persister")
        .add_edge("persister", "gateway")
        // loop_target 没有出边，消息到达后工作流自然结束
        .add_loopback_edge("gateway", "loop_target")
        .build()
        .expect("build graph");

    let runtime = WorkflowRuntime::start(graph, Arc::new("start".to_string()), None)
        .await
        .expect("start runtime");

    let mut events = runtime.events().await.expect("events");
    let mut completed = false;
    let mut loop_target_invoked = false;

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                match ev {
                    WorkflowEvent::NodeInvoking { node_id, .. } => {
                        if node_id == "loop_target" {
                            loop_target_invoked = true;
                        }
                    }
                    WorkflowEvent::WorkflowCompleted { .. } => {
                        completed = true;
                        break;
                    }
                    _ => {}
                }
            }
            _ = &mut timeout => break,
        }
    }

    assert!(completed, "工作流应该完成");
    assert!(
        loop_target_invoked,
        "审查未通过时应触发 loop_target（回环）"
    );

    let _ = runtime.wait().await;
}

#[test]
fn test_review_verdict_parsing() {
    // 验证 ReviewVerdict 解析逻辑
    let passed = r#"{"passed": true}"#;
    let verdict = ReviewVerdict::parse_from_text(passed).expect("parse");
    assert!(verdict.passed);

    let failed = r#"```json
    {"passed": false, "discrepancies": ["问题1"]}
    ```"#;
    let verdict = ReviewVerdict::parse_from_text(failed).expect("parse");
    assert!(!verdict.passed);
    assert_eq!(verdict.discrepancies.len(), 1);
}

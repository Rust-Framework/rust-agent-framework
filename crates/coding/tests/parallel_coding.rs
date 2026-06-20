//! 测试 FanOut/FanIn 并行编码 — 验证并行执行与消息合并。

use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::ChatMessage;
use rust_agent_workflow::{
    ContextFunctionExecutor, FunctionExecutor, HandlerResult, IExecutor, WorkflowBuilder,
    WorkflowEvent, WorkflowRuntime,
};

/// 简化的合并器 — 使用状态计数器等待两条消息后合并。
fn simple_merger(node_id: &str) -> Arc<dyn IExecutor> {
    let node_id = node_id.to_string();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        move |_msg, ctx, _progress| async move {
            const COUNT: &str = "merger_count";
            let count = match ctx.read_state(COUNT).await.unwrap_or(None) {
                Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
                _ => 0,
            };
            let next = count + 1;
            ctx.write_state(
                COUNT,
                serde_json::Value::Number(serde_json::Number::from(next)),
            )
            .await
            .unwrap();

            if next < 2 {
                return Ok(HandlerResult::None);
            }

            ctx.write_state(
                COUNT,
                serde_json::Value::Number(serde_json::Number::from(0)),
            )
            .await
            .unwrap();

            let alpha = match ctx.read_state("alpha").await.unwrap_or(None) {
                Some(serde_json::Value::String(s)) => s,
                _ => String::new(),
            };
            let beta = match ctx.read_state("beta").await.unwrap_or(None) {
                Some(serde_json::Value::String(s)) => s,
                _ => String::new(),
            };

            let merged = format!("alpha={}, beta={}", alpha, beta);
            Ok(HandlerResult::Messages(vec![Arc::new(
                ChatMessage::assistant(&merged),
            )]))
        },
    ))
}

#[tokio::test]
async fn test_fanout_fanin_parallel_coding() {
    // 构建图: entry → FanOut(alpha, beta) → FanIn(merger) → output
    let graph = WorkflowBuilder::new()
        .add_node(
            "entry",
            Arc::new(FunctionExecutor::new("entry", |msg: String| vec![msg])),
        )
        // alpha 路径
        .add_node(
            "alpha_inject",
            Arc::new(ContextFunctionExecutor::new(
                "alpha_inject",
                |_msg, ctx, _progress| async move {
                    ctx.write_state("alpha", serde_json::Value::String("alpha-output".into()))
                        .await
                        .unwrap();
                    Ok(HandlerResult::Messages(vec![Arc::new(
                        ChatMessage::assistant("alpha-output"),
                    )]))
                },
            )),
        )
        // beta 路径
        .add_node(
            "beta_inject",
            Arc::new(ContextFunctionExecutor::new(
                "beta_inject",
                |_msg, ctx, _progress| async move {
                    ctx.write_state("beta", serde_json::Value::String("beta-output".into()))
                        .await
                        .unwrap();
                    Ok(HandlerResult::Messages(vec![Arc::new(
                        ChatMessage::assistant("beta-output"),
                    )]))
                },
            )),
        )
        .add_node("merger", simple_merger("merger"))
        .add_node(
            "output",
            Arc::new(FunctionExecutor::new("output", |msg: ChatMessage| {
                vec![msg]
            })),
        )
        .set_start("entry")
        .add_fan_out_edge("entry", vec!["alpha_inject", "beta_inject"])
        .add_fan_in_edge(vec!["alpha_inject", "beta_inject"], "merger")
        .add_edge("merger", "output")
        .with_output_from("output")
        .build()
        .expect("build graph");

    let runtime = WorkflowRuntime::start(graph, Arc::new("start".to_string()), None)
        .await
        .expect("start runtime");

    let mut events = runtime.events().await.expect("events");
    let mut completed = false;

    let timeout = tokio::time::sleep(std::time::Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                if matches!(ev, WorkflowEvent::WorkflowCompleted { .. }) {
                    completed = true;
                    break;
                }
            }
            _ = &mut timeout => break,
        }
    }

    assert!(completed, "并行编码应该完成");

    // 验证输出
    if let Some(mut outputs) = runtime.outputs().await {
        while let Some(Ok(output)) = outputs.next().await {
            if let Some(msg) = output.content.downcast_ref::<ChatMessage>() {
                assert!(
                    msg.content.contains("alpha-output"),
                    "合并结果应包含 alpha 输出: {}",
                    msg.content
                );
                assert!(
                    msg.content.contains("beta-output"),
                    "合并结果应包含 beta 输出: {}",
                    msg.content
                );
            }
        }
    }

    let _ = runtime.wait().await;
}

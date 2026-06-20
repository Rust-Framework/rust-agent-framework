//! 真实 LLM 集成测试 — 验证 coding crate 与 DeepSeek API 的端到端连通性。
//!
//! 这些测试需要真实 API key，默认被忽略（`#[ignore]`）。
//! 运行方式：
//! ```bash
//! DEEPSEEK_API_KEY=sk-xxx cargo test -p rust-agent-coding --test real_llm_smoke -- --ignored --nocapture
//! ```
//!
//! 若未设置 `DEEPSEEK_API_KEY` 环境变量，将回退到 cli crate 的 `cli-agent.yaml` 中配置的 key。

use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_client::ChatClientOptions;
use rust_agent_coding::{
    agents::create_requirements_analyst,
    executors::{artifact_persist, context_inject},
    state::state_keys,
};
use rust_agent_core::ChatMessage;
use rust_agent_workflow::{
    AgentExecutor, ContextFunctionExecutor, HandlerResult, IExecutor, WorkflowBuilder,
    WorkflowEvent, WorkflowOutput, WorkflowRuntime,
};

/// 从环境变量或 cli-agent.yaml 回退获取 API key。
fn resolve_api_key() -> String {
    if let Ok(key) = std::env::var("DEEPSEEK_API_KEY") {
        return key;
    }
    // 回退到 cli crate 的 cli-agent.yaml 中配置的 key
    "sk-b8136a230aea467e8cdfe4649cab2d3e".to_string()
}

/// 构建一个调用 `yield_output` 的终点节点。
fn output_node(node_id: &str) -> Arc<dyn IExecutor> {
    let node_id = node_id.to_string();
    Arc::new(ContextFunctionExecutor::new(
        node_id,
        |msg, ctx, _progress| async move {
            ctx.yield_output(msg.clone()).await?;
            Ok(HandlerResult::None)
        },
    ))
}

/// 真实 LLM 冒烟测试 — 运行阶段 1 需求分析，验证 LLM 端到端连通性。
///
/// 构建简化图：p1_inject → p1_analyst → p1_persist → output
/// 验证：
/// 1. 工作流正常完成（非超时）
/// 2. LLM 返回非空的需求分析响应
/// 3. 响应内容包含需求分析关键词（如"需求"、"接口"或"功能"等）
#[tokio::test]
#[ignore]
async fn test_real_requirements_analysis() {
    let api_key = resolve_api_key();
    let options = ChatClientOptions::deepseek("deepseek-v4-flash", api_key);
    let workspace_root = std::env::temp_dir();

    // 构建简化图：p1_inject → p1_analyst → p1_persist → output
    let analyst =
        create_requirements_analyst(&options, &workspace_root).expect("创建需求分析 Agent 失败");
    let inject = context_inject(
        "p1_inject",
        vec![],
        "请根据以下用户需求进行全面的需求分解：\n\n{artifacts}\n\n（如果上方为空，请基于初始消息分析）"
            .to_string(),
    );
    let persist = artifact_persist("p1_persist", state_keys::REQUIREMENTS_DOC, None);
    let output = output_node("output");

    let graph = WorkflowBuilder::new()
        .add_node("p1_inject", inject)
        .add_node(
            "p1_analyst",
            Arc::new(AgentExecutor::new("p1_analyst", analyst)),
        )
        .add_node("p1_persist", persist)
        .add_node("output", output)
        .set_start("p1_inject")
        .add_edge("p1_inject", "p1_analyst")
        .add_edge("p1_analyst", "p1_persist")
        .add_edge("p1_persist", "output")
        .build()
        .expect("构建工作流图失败");

    // 启动 runtime
    let runtime = WorkflowRuntime::start(
        graph,
        Arc::new(ChatMessage::user(
            "实现一个简单的 TODO 待办事项应用，支持增删改查和标记完成",
        )),
        None,
    )
    .await
    .expect("启动 runtime 失败");

    let mut events = runtime.events().await.expect("获取事件流失败");
    let mut outputs = runtime.outputs().await.expect("获取输出流失败");
    let mut completed = false;
    let mut agent_response_text = String::new();
    let mut error_message = String::new();

    // 120 秒超时（LLM 调用可能较慢）
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(120));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Some(ev) = events.next() => {
                match ev {
                    WorkflowEvent::NodeInvoking { node_id, .. } => {
                        println!("[节点] {} 开始执行", node_id);
                    }
                    WorkflowEvent::NodeCompleted { node_id, .. } => {
                        println!("[节点] {} 完成", node_id);
                    }
                    WorkflowEvent::NodeFailed { node_id, error } => {
                        eprintln!("[节点] {} 失败: {}", node_id, error);
                        error_message = format!("节点 {} 失败: {}", node_id, error);
                        break;
                    }
                    WorkflowEvent::WorkflowCompleted { .. } => {
                        println!("[工作流] 完成");
                        completed = true;
                        break;
                    }
                    WorkflowEvent::WorkflowError { error, .. } => {
                        eprintln!("[工作流] 错误: {}", error);
                        error_message = format!("工作流错误: {}", error);
                        break;
                    }
                    _ => {}
                }
            }
            Some(output) = outputs.next() => {
                match output {
                    Ok(WorkflowOutput { content, source_node_id }) => {
                        println!("[输出] 来自节点: {}", source_node_id);
                        // 取 "output" 节点的输出（即 LLM 响应）
                        if source_node_id == "output" {
                            if let Some(msg) = content.downcast_ref::<ChatMessage>() {
                                agent_response_text = msg.content.clone();
                                println!("[输出] LLM 响应 {} 字符", agent_response_text.len());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[输出] 错误: {}", e);
                    }
                }
            }
            _ = &mut timeout => {
                panic!("测试超时（120秒）— LLM 未在预期时间内返回响应");
            }
        }
    }

    let _ = runtime.wait().await;

    // 断言验证
    assert!(
        error_message.is_empty(),
        "工作流执行出错: {}",
        error_message
    );
    assert!(completed, "工作流应该正常完成");
    assert!(!agent_response_text.is_empty(), "应该收到 LLM 的非空响应");
    assert!(
        agent_response_text.len() > 100,
        "需求分析响应应该足够详细（至少 100 字符），实际: {} 字符",
        agent_response_text.len()
    );

    // 验证响应内容包含需求分析相关关键词
    let response_lower = agent_response_text.to_lowercase();
    let has_requirement_keyword = response_lower.contains("需求")
        || response_lower.contains("功能")
        || response_lower.contains("接口")
        || response_lower.contains("用户")
        || response_lower.contains("requirement");
    let preview: String = agent_response_text.chars().take(200).collect();
    assert!(
        has_requirement_keyword,
        "响应应包含需求分析相关关键词，实际内容前 200 字符: {}",
        preview
    );

    let preview_500: String = agent_response_text.chars().take(500).collect();
    println!("\n=== 需求分析结果（前 500 字符）===");
    println!("{}", preview_500);
    println!("\n=== 测试通过：真实 LLM 需求分析验证成功 ===");
}

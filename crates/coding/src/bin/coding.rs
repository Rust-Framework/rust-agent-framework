//! 交互式 CLI 入口 — 演示 6 阶段开发流水线的完整 HITL 流程。
//!
//! 用法：
//! ```bash
//! export AGNES_API_KEY=your_key
//! cargo run -p rust-agent-coding -- "实现一个 TODO 应用"
//! ```

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_client::ChatClientOptions;
use rust_agent_coding::build_dev_pipeline;
use rust_agent_core::ChatMessage;
use rust_agent_workflow::{ResumeCommand, WorkflowEvent, WorkflowRuntime};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // ── 解析参数 ──
    let api_key = std::env::var("AGNES_API_KEY")
        .map_err(|_| anyhow::anyhow!("请设置 AGNES_API_KEY 环境变量"))?;
    let initial_requirement = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("用法: coding <需求描述>");
        eprintln!("请输入需求描述:");
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).ok();
        input.trim().to_string()
    });

    if initial_requirement.is_empty() {
        anyhow::bail!("需求描述不能为空");
    }

    let mut options = ChatClientOptions::openai("agnes-2.0-flash", api_key);
    options.api_base = "https://apihub.agnes-ai.com/v1".to_string();
    let workspace_root = std::env::current_dir()?;

    println!("=== 6 阶段开发流水线启动 ===");
    println!("需求: {}", initial_requirement);
    println!("工作区: {}", workspace_root.display());
    println!();

    // ── 构建工作流图 ──
    let graph = build_dev_pipeline(&options, &workspace_root)?;

    // ── 启动 runtime ──
    let runtime = WorkflowRuntime::start(
        graph,
        Arc::new(ChatMessage::user(&initial_requirement)),
        None,
    )
    .await?;

    // ── 事件循环 ──
    let mut events = runtime.events().await.expect("events");
    let mut last_node_id = String::new();

    while let Some(ev) = events.next().await {
        match ev {
            WorkflowEvent::NodeInvoking { node_id, .. } => {
                println!("[节点] {} 开始执行", node_id);
                last_node_id = node_id;
            }
            WorkflowEvent::NodeCompleted { node_id, .. } => {
                println!("[节点] {} 完成", node_id);
            }
            WorkflowEvent::NodeFailed { node_id, error } => {
                eprintln!("[节点] {} 失败: {}", node_id, error);
            }
            WorkflowEvent::Custom { key, data } if key == "halt_payload" => {
                println!("\n=== 人机确认 ===");
                if let Some(task) = data.get("task").and_then(|v| v.as_str()) {
                    println!("任务: {}", task);
                }
                if let Some(instr) = data.get("instruction").and_then(|v| v.as_str()) {
                    println!("说明: {}", instr);
                }
                println!();

                // 读取用户输入
                let user_input = read_user_input("请输入确认或修改建议（直接回车表示确认）: ")?;
                let confirmation = if user_input.trim().is_empty() {
                    "确认".to_string()
                } else {
                    user_input
                };

                // 恢复工作流
                runtime.resume(ResumeCommand::InjectMessage {
                    target_node_id: last_node_id.clone(),
                    message: Arc::new(confirmation),
                })?;
                println!();
            }
            WorkflowEvent::WorkflowHalted { .. } => {
                println!("[工作流] 暂停，等待人工确认...");
            }
            WorkflowEvent::WorkflowResumed { .. } => {
                println!("[工作流] 已恢复");
            }
            WorkflowEvent::WorkflowCompleted { .. } => {
                println!("\n=== 工作流完成 ===");
                break;
            }
            WorkflowEvent::WorkflowError { error, node_id } => {
                eprintln!("\n[错误] {}", error);
                if let Some(nid) = node_id {
                    eprintln!("  节点: {}", nid);
                }
                break;
            }
            _ => {}
        }
    }

    runtime.wait().await?;
    println!("流水线执行结束。");
    Ok(())
}

/// 读取用户输入
fn read_user_input(prompt: &str) -> anyhow::Result<String> {
    print!("{}", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

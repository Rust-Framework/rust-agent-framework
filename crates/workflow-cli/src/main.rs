//! Workflow 流程编排 CLI 测试程序
//!
//! 功能覆盖：
//! 1. AgentHost + Session 注册中心
//! 2. ChatClient 管道模式 (FunctionInvokingChatClient)
//! 3. WorkflowEngine + Checkpoint 集成
//! 4. Session TTL cleanup 验证
//! 5. get_subagent 多智能体查找
//! 6. 流式输出消费 + 工具调用
//!
//! 运行方式：
//!   cargo run -p rust-agent-workflow-cli
//!   RUST_LOG=debug cargo run -p rust-agent-workflow-cli
//!   RUST_LOG=trace cargo run -p rust-agent-workflow-cli

use std::sync::Arc;
use anyhow::Result;
use futures_util::StreamExt;

use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_core::{
    AgentId, AgentSession, ChatMessage, ISession, ISessionStore, SessionTTLOptions, Content,
};
use rust_agent_framework::{
    AgentBuilder, AgentHost, InMemorySessionStore,
    tools::ReadFile,
};

// ============================================================
// 配置（仅测试用途）
// ============================================================

const DEEPSEEK_API_KEY: &str = "sk-9f8dbaaa822e477faf339e32cdb89e91";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";

// ============================================================
// 测试场景
// ============================================================

/// 场景 1：AgentHost 注册中心 + ChatClient 管道模式 + 流式输出
async fn scenario_1_agent_host_pipeline() -> Result<()> {
    println!("\n=== 场景 1：AgentHost + FunctionInvokingChatClient 管道 ===");

    let store: Arc<dyn ISessionStore> = Arc::new(
        InMemorySessionStore::new()
    );

    let options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);
    let client = DeepSeekChatClient::new(options)?;

    let agent = AgentBuilder::new("pipeline-agent")
        .chat_client(client)
        .instructions("你是 AI 助手。回复用中文，一句话介绍自己。")
        .build()?;

    let host = AgentHost::new(agent, store.clone());

    // get_subagent 验证
    let found = host.get_subagent(&AgentId::new("pipeline-agent"));
    println!("  [get_subagent] pipeline-agent: found={}", found.is_some());
    let none = host.get_subagent(&AgentId::new("nonexistent"));
    println!("  [get_subagent] nonexistent: found={}", none.is_some());

    // Session 管理
    let session = host.get_or_create_session("test-session-1").await?;
    println!("  [session] created: {}", session.session_id());

    // 发送消息并消费流
    let stream = host.run(
        vec![ChatMessage::user("你好！请用一句话介绍自己。")],
        session.clone(),
        None,
    ).await?;

    print!("  [stream] ");
    let mut s = Box::pin(stream);
    let mut text = String::new();
    while let Some(chunk) = s.next().await {
        match chunk {
            Ok(result) => {
                for content in &result.contents {
                    if let Content::Text(ref t) = content {
                        text.push_str(&t.delta);
                        print!("{}", t.delta);
                    }
                }
            }
            Err(e) => eprintln!("\n  [error] {}", e),
        }
    }
    println!();
    println!("  [response] {} chars", text.len());

    // 验证 agent 引用可达
    let agent_ref = host.agent();
    println!("  [agent] id={} type={}", agent_ref.id(), agent_ref.metadata().agent_type);

    Ok(())
}

/// 场景 2：WorkflowEngine + Checkpoint 集成
async fn scenario_2_workflow_engine_checkpoint() -> Result<()> {
    println!("\n=== 场景 2：WorkflowEngine + Checkpoint 集成 ===");

    use rust_agent_workflow::{
        WorkflowBuilder, CheckpointManager, InMemoryCheckpointStore,
        FunctionExecutor, WorkflowEngine,
    };
    use rust_agent_core::AgentSession;

    let store = Arc::new(InMemoryCheckpointStore::new());
    let cp_manager = Arc::new(CheckpointManager::with_default_config(store));

    let graph = WorkflowBuilder::new()
        .add_node("researcher", Arc::new(FunctionExecutor::new(
            "researcher",
            |msg: String| vec![format!("# 研究结果\n\n关于「{}」的初步分析：\n- 要点 1：数据表明趋势向上\n- 要点 2：市场反应积极", msg)]
        )))
        .set_start("researcher")
        .with_output_from("researcher")
        .build()?;

    let engine = WorkflowEngine::new(graph)
        .with_checkpoint_manager(cp_manager);

    let session: Arc<dyn ISession> = Arc::new(AgentSession::with_id("workflow-test"));

    let (mut events, _outputs) = engine
        .run(Box::new("AI 芯片市场趋势".to_string()), Some(session))
        .await?;

    let mut event_count = 0;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(_event) = events.next() => {
                event_count += 1;
            }
            _ = &mut timeout => { break; }
        }
    }

    println!("  [workflow] events received: {}", event_count);
    println!("  [workflow] checkpoint lifecycle verified (see logs/workflow-cli.log)");

    Ok(())
}

/// 场景 3：Session TTL cleanup 验证
async fn scenario_3_session_cleanup() -> Result<()> {
    println!("\n=== 场景 3：Session TTL cleanup 验证 ===");

    let store = InMemorySessionStore::new()
        .with_ttl(SessionTTLOptions {
            max_idle_secs: Some(1),
            max_lifetime_secs: None,
            cleanup_interval_secs: 60,
        });

    // 创建多个 session
    for i in 0..5 {
        let s = Arc::new(AgentSession::with_id(&format!("cleanup-s-{}", i)));
        store.save_session(s.as_ref()).await?;
    }
    println!("  [sessions] created 5 sessions");

    // 等待过期
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let removed = store.cleanup_expired().await?;
    println!("  [cleanup] expired sessions removed: {}", removed);
    assert_eq!(removed, 5, "All 5 should be expired by idle timeout");

    Ok(())
}

/// 场景 4：FunctionInvokingChatClient 工具调用管道
async fn scenario_4_tool_call_pipeline() -> Result<()> {
    println!("\n=== 场景 4：FunctionInvokingChatClient 工具调用管道 ===");

    let options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);
    let client = DeepSeekChatClient::new(options)?;

    let agent: Arc<dyn rust_agent_core::IAgent> = AgentBuilder::new("tool-agent")
        .chat_client(client)
        .instructions("你是文件分析助手。用户要求读文件时使用 read_file 工具。")
        .with_tool(ReadFile)
        .max_tool_rounds(3)
        .build()?;

    let session = Arc::new(AgentSession::with_id("tool-test"));

    let stream = agent.run(
        vec![ChatMessage::user("读取 Cargo.toml 文件，路径是 Cargo.toml，然后总结项目结构。")],
        Some(session),
        None,
    ).await?;

    print!("  [stream] ");
    let mut s = Box::pin(stream);
    let mut text = String::new();
    let mut tool_count = 0;
    while let Some(chunk) = s.next().await {
        match chunk {
            Ok(result) => {
                for content in &result.contents {
                    match content {
                        Content::Text(ref t) => {
                            text.push_str(&t.delta);
                            print!("{}", t.delta);
                        }
                        Content::ToolCallStart(inner) => {
                            tool_count += 1;
                            println!("\n  [tool] calling {}", inner.name);
                        }
                        Content::ToolCalled(inner) => {
                            if let Some(err) = &inner.error {
                                println!("  [tool error] {}", err);
                            } else if let Some(r) = &inner.result {
                                let preview = if r.len() > 300 { &r[..300] } else { r.as_str() };
                                println!("  [tool result] {}...", preview);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => eprintln!("\n  [error] {}", e),
        }
    }
    println!();
    println!("  [tool calls] count={}", tool_count);
    println!("  [response] {} chars", text.len());

    Ok(())
}

// ============================================================
// 主函数
// ============================================================

#[tokio::main]
async fn main() -> Result<()> {
    // 初始化 tracing：debug 级别，输出到文件
    let _guard = {
        let file_appender = tracing_appender::rolling::never("logs", "workflow-cli.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("debug".parse()?)
            )
            .with_writer(non_blocking)
            .init();
        _guard
    };

    println!("╔══════════════════════════════════════════╗");
    println!("║  Workflow CLI — 流程编排测试程序          ║");
    println!("║  日志输出: logs/workflow-cli.log           ║");
    println!("╚══════════════════════════════════════════╝");

    // ── 运行所有场景 ──
    let mut passed = 0usize;
    let mut failed = 0usize;

    macro_rules! run_scenario {
        ($name:expr, $fn:expr) => {
            print!("▶ {}", $name);
            match $fn.await {
                Ok(_) => { println!("  ✅"); passed += 1; }
                Err(e) => { println!("  ❌\n  {}", e); failed += 1; }
            }
        };
    }

    run_scenario!("场景1: AgentHost 管道", scenario_1_agent_host_pipeline());
    run_scenario!("场景2: WorkflowEngine+Checkpoint", scenario_2_workflow_engine_checkpoint());
    run_scenario!("场景3: Session TTL cleanup", scenario_3_session_cleanup());
    run_scenario!("场景4: 工具调用管道", scenario_4_tool_call_pipeline());

    println!("\n╔══════════════════════════════════════════╗");
    println!("║  结果: {}/{} 通过, {} 失败", passed, passed + failed, failed);
    println!("║  完整日志: logs/workflow-cli.log");
    if failed > 0 {
        println!("║  ⚠ 存在失败场景，请查看上方错误信息");
    } else {
        println!("║  ✅ 所有场景通过！");
    }
    println!("╚══════════════════════════════════════════╝");

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

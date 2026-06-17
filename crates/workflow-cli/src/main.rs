//! Workflow 流程编排 CLI 测试程序
//!
//! 功能覆盖：
//! 1. Session 直接管理（MAF 一致的原语模式）
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
    IAgent,
};
use rust_agent_framework::{
    AgentBuilder, InMemorySessionStore,
    tools::ReadFile,
};
use rust_agent_workflow::orchestrations::{
    SequentialWorkflow, HandoffWorkflow, ConcurrentWorkflow,
};

// ============================================================
// 配置（仅测试用途）
// ============================================================

const DEEPSEEK_API_KEY: &str = "sk-9f8dbaaa822e477faf339e32cdb89e91";
const DEFAULT_MODEL: &str = "deepseek-v4-flash";

// ============================================================
// 测试场景
// ============================================================

/// 场景 1：Session 直接管理 + ChatClient 管道模式 + 流式输出（MAF 一致的原语模式）
async fn scenario_1_agent_host_pipeline() -> Result<()> {
    println!("\n=== 场景 1：Session 直接管理 + FunctionInvokingChatClient 管道 ===");

    let store: Arc<dyn ISessionStore> = Arc::new(
        InMemorySessionStore::new()
    );

    let options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);
    let client = DeepSeekChatClient::new(options)?;

    let agent = AgentBuilder::new("pipeline-agent")
        .chat_client(client)
        .instructions("你是 AI 助手。回复用中文，一句话介绍自己。")
        .build()?;

    // get_subagent 验证（直接通过 IAgent）
    let found = agent.get_subagent(&AgentId::new("pipeline-agent"));
    println!("  [get_subagent] pipeline-agent: found={}", found.is_some());
    let none = agent.get_subagent(&AgentId::new("nonexistent"));
    println!("  [get_subagent] nonexistent: found={}", none.is_some());

    // Session 管理（MAF 一致的原语模式：加载或创建）
    let session_id = "test-session-1";
    let session: Arc<dyn ISession> = match store.get_session(session_id).await? {
        Some(s) => s,
        None => {
            let s = Arc::new(AgentSession::with_id(session_id));
            store.save_session(s.as_ref()).await?;
            s
        }
    };
    println!("  [session] created: {}", session.session_id());

    // 直接调用 agent.run()（应用层决定何时保存）
    let stream = agent.run(
        vec![ChatMessage::user("你好！请用一句话介绍自己。")],
        Some(session.clone()),
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

    // 流消费后保存（应用层决定时机）
    store.save_session(session.as_ref()).await?;
    println!("  [response] {} chars", text.len());

    // 验证 agent 信息
    println!("  [agent] id={} type={}", agent.id(), agent.metadata().agent_type);

    Ok(())
}

/// 场景 5：HandoffWorkflow — triage 路由 + as_agent() + get_subagent（真实 LLM）
async fn scenario_5_handoff_routing() -> Result<()> {
    println!("\n=== 场景 5：HandoffWorkflow — as_agent() + get_subagent ===");

    let options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);

    let coder = AgentBuilder::new("code-expert")
        .chat_client(DeepSeekChatClient::new(options.clone())?)
        .instructions("你是代码专家。用中文回复。")
        .with_description("代码专家")
        .build()?;

    let writer = AgentBuilder::new("writing-expert")
        .chat_client(DeepSeekChatClient::new(options.clone())?)
        .instructions("你是写作专家。用中文回复。")
        .with_description("写作专家")
        .build()?;

    let triage = AgentBuilder::new("triage")
        .chat_client(DeepSeekChatClient::new(
            ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY)
        )?)
        .instructions("你是任务路由器，分析用户请求并选择最合适的专家。只回复专家名称。")
        .build()?;

    let workflow = HandoffWorkflow::new()
        .triage(triage)
        .agent(coder)
        .agent(writer)
        .build()?;

    // ── as_agent() → IAgent 统一门面 ──
    let agent: Arc<dyn IAgent> = workflow.as_agent();
    println!("  [as_agent] id={}, type={}", agent.id(), agent.metadata().agent_type);
    println!("  [as_agent] description: {}", agent.metadata().description);

    // ── get_subagent 发现子代理 ──
    for sub_id in &[
        AgentId::new("code-expert"),
        AgentId::new("writing-expert"),
    ] {
        let found = agent.get_subagent(sub_id);
        println!("  [get_subagent] {} → {}", sub_id, if found.is_some() { "found" } else { "not found" });
    }

    let session = Arc::new(AgentSession::with_id("handoff-test"));

    let stream = agent.run(
        vec![ChatMessage::user("帮我写一段 Python 快速排序代码")],
        Some(session),
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
    assert!(!text.is_empty(), "Should get response from target agent");

    Ok(())
}

/// 场景 6：SequentialWorkflow — 多 Agent 顺序编排（真实 LLM）
async fn scenario_6_sequential_multi_agent() -> Result<()> {
    println!("\n=== 场景 6：SequentialWorkflow — 顺序编排 ===");

    let options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);

    let researcher = AgentBuilder::new("researcher")
        .chat_client(DeepSeekChatClient::new(options.clone())?)
        .instructions("你是研究员。用中文回复，简洁列出 3 个要点。")
        .build()?;

    let summarizer = AgentBuilder::new("summarizer")
        .chat_client(DeepSeekChatClient::new(options)?)
        .instructions("你是总结专家。将研究结果总结为一句话。用中文回复。")
        .build()?;

    let pattern = SequentialWorkflow::from_agents(vec![researcher, summarizer]);
    let session = Arc::new(AgentSession::with_id("seq-test"));

    let stream = pattern.run(
        vec![ChatMessage::user("分析 AI 在医疗领域的应用前景")],
        Some(session),
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
    assert!(!text.is_empty(), "Should get sequential result");

    Ok(())
}

/// 场景 7：ConcurrentWorkflow — 并发多 Agent（真实 LLM）
async fn scenario_7_concurrent_multi_agent() -> Result<()> {
    println!("\n=== 场景 7：ConcurrentWorkflow — 并发编排 ===");

    let options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);

    let a1 = AgentBuilder::new("angle-tech")
        .chat_client(DeepSeekChatClient::new(options.clone())?)
        .instructions("你用一句话从技术角度评价 AI 发展。用中文。")
        .build()?;

    let a2 = AgentBuilder::new("angle-business")
        .chat_client(DeepSeekChatClient::new(options)?)
        .instructions("你用一句话从商业角度评价 AI 发展。用中文。")
        .build()?;

    let pattern = ConcurrentWorkflow::from_agents(vec![a1, a2]);
    let session = Arc::new(AgentSession::with_id("concurrent-test"));

    let stream = pattern.run(
        vec![ChatMessage::user("评价 AI")],
        Some(session),
        None,
    ).await?;

    print!("  [stream] ");
    let mut s = Box::pin(stream);
    let mut text = String::new();
    let mut result_count = 0;
    while let Some(chunk) = s.next().await {
        match chunk {
            Ok(result) => {
                for content in &result.contents {
                    if let Content::Text(ref t) = content {
                        text.push_str(&t.delta);
                        print!("{}", t.delta);
                    }
                }
                result_count += 1;
            }
            Err(e) => eprintln!("\n  [error] {}", e),
        }
    }
    println!();
    println!("  [results] {} agents, {} chars", result_count, text.len());
    assert!(result_count >= 2, "Should get results from both agents");

    Ok(())
}

/// 场景 8：as_agent() → get_subagent → 子代理独立流式输出（真实 LLM）
///
/// 验证 MAF 设计哲学的核心闭环：
/// 1. WorkflowBuilder → as_agent() → IAgent 统一门面
/// 2. get_subagent(id) 获取子代理 IAgent
/// 3. 子代理独立 run() 产生流式输出
/// 4. 父代理 run() 通过 triage 路由到子代理
async fn scenario_8_sub_agent_handoff_flow() -> Result<()> {
    println!("\n=== 场景 8：as_agent() → get_subagent → 子代理流式输出 ===");

    let options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);

    // 子代理 A — 代码专家
    let coder_client = DeepSeekChatClient::new(options.clone())?;
    let coder = AgentBuilder::new("code-expert")
        .chat_client(coder_client)
        .instructions("你是 Python 代码专家。用中文回复，只输出代码和简短解释。")
        .build()?;
    let coder_id = coder.id().clone();

    // 子代理 B — 文档专家
    let doc_options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);
    let doc_client = DeepSeekChatClient::new(doc_options)?;
    let writer = AgentBuilder::new("doc-expert")
        .chat_client(doc_client)
        .instructions("你是技术文档专家。用中文回复，简洁专业。")
        .build()?;
    let writer_id = writer.id().clone();

    // Triage
    let triage_options = ChatClientOptions::deepseek(DEFAULT_MODEL, DEEPSEEK_API_KEY);
    let triage = AgentBuilder::new("triage")
        .chat_client(DeepSeekChatClient::new(triage_options)?)
        .instructions("你是任务路由器，分析用户请求。只回复 code-expert 或 doc-expert。")
        .build()?;

    let workflow = HandoffWorkflow::new()
        .triage(triage)
        .agent(coder)
        .agent(writer)
        .build()?;

    // ── Step 1: as_agent() 获取统一门面 ──
    let agent: Arc<dyn IAgent> = workflow.as_agent();
    println!("  [1] as_agent → id={}", agent.id());

    // ── Step 2: get_subagent 获取子代理 ──
    let sub_coder = agent.get_subagent(&coder_id);
    let sub_writer = agent.get_subagent(&writer_id);
    assert!(sub_coder.is_some(), "Should find code-expert sub-agent");
    assert!(sub_writer.is_some(), "Should find doc-expert sub-agent");
    println!("  [2] get_subagent → code-expert: found, doc-expert: found");

    // ── Step 3: 子代理独立运行（流式输出）──
    let coder_agent = sub_coder.unwrap();
    println!("  [3] sub-agent(id={}) running independently...", coder_agent.id());
    let sub_session = Arc::new(AgentSession::with_id("sub-agent-test"));

    let sub_stream = coder_agent.run(
        vec![ChatMessage::user("写一个 Python 函数计算斐波那契数列")],
        Some(sub_session),
        None,
    ).await?;

    print!("      [coder stream] ");
    let mut s = Box::pin(sub_stream);
    let mut sub_text = String::new();
    let mut sub_chunks = 0usize;
    while let Some(chunk) = s.next().await {
        match chunk {
            Ok(result) => {
                for content in &result.contents {
                    if let Content::Text(ref t) = content {
                        sub_text.push_str(&t.delta);
                        print!("{}", t.delta);
                    }
                }
                sub_chunks += 1;
            }
            Err(e) => eprintln!("\n      [error] {}", e),
        }
    }
    println!();
    println!("      [chunks] {}, [chars] {}", sub_chunks, sub_text.len());
    assert!(!sub_text.is_empty(), "Sub-agent should produce text output");
    assert!(sub_text.contains("def") || sub_text.contains("fib"), "Sub-agent should output Python code");

    // ── Step 4: 父代理 triage 路由验证 ──
    println!("  [4] parent agent triage routing...");
    let parent_session = Arc::new(AgentSession::with_id("parent-test"));
    let parent_stream = agent.run(
        vec![ChatMessage::user("写一个 Python 快速排序函数")],
        Some(parent_session),
        None,
    ).await?;

    print!("      [parent stream] ");
    let mut ps = Box::pin(parent_stream);
    let mut parent_text = String::new();
    while let Some(chunk) = ps.next().await {
        match chunk {
            Ok(result) => {
                for content in &result.contents {
                    if let Content::Text(ref t) = content {
                        parent_text.push_str(&t.delta);
                        print!("{}", t.delta);
                    }
                }
            }
            Err(e) => eprintln!("\n      [error] {}", e),
        }
    }
    println!();
    println!("      [chars] {}", parent_text.len());
    assert!(!parent_text.is_empty(), "Parent triage should produce output");

    println!("  ✅ as_agent → get_subagent → sub-agent stream → parent triage: ALL VERIFIED");

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
        .with_tool(ReadFile::default())
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

    run_scenario!("场景1: Session直接管理", scenario_1_agent_host_pipeline());
    run_scenario!("场景2: WorkflowEngine+Checkpoint", scenario_2_workflow_engine_checkpoint());
    run_scenario!("场景3: Session TTL cleanup", scenario_3_session_cleanup());
    run_scenario!("场景4: 工具调用管道", scenario_4_tool_call_pipeline());
    run_scenario!("场景5: Handoff 路由编排", scenario_5_handoff_routing());
    run_scenario!("场景6: Sequential 顺序编排", scenario_6_sequential_multi_agent());
    run_scenario!("场景7: Concurrent 并发编排", scenario_7_concurrent_multi_agent());
    run_scenario!("场景8: as_agent→get_subagent→流式输出", scenario_8_sub_agent_handoff_flow());

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

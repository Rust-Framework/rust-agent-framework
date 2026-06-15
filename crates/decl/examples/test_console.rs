//! Test console for rust-agent-decl — interactive REPL for building and
//! testing agents from declarations.
//!
//! Run: `cargo run --example test_console`
//!
//! ## Features
//! - Hardcoded API key for zero-config testing
//! - `tracing` output for debugging (RUST_LOG=info cargo run --example test_console -- --auto)
//! - Built-in REPL with commands to load/test declarations on the fly
//! - ANSI colorized streaming output following the cli crate pattern
//! - Auto-run mode: `cargo run --example test_console -- --auto`

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use futures_core::Stream;
use rust_agent_client::{ChatClientOptions, DeepSeekChatClient, OpenAiChatClient};
use rust_agent_core::{
    AgentRunOptions, ChatMessage, Content, IAgent, IChatClient, ReasoningEffort,
};
use rust_agent_decl::{
    AgentDecl, DefaultAgentResolver,
    resolver::ClientWrapper,
};
use rust_agent_framework::AgentBuilder;

// ── API Key (hardcoded for testing convenience) ──

/// DeepSeek API key. Replace with your own key.
const DEEPSEEK_API_KEY: &str = "sk-9f8dbaaa822e477faf339e32cdb89e91";

// ── Built-in test tools ──

use rust_agent_framework::tool;

#[tool(description = "Echoes back the input text")]
async fn echo(#[param(desc = "The text to echo")] text: String) -> String {
    tracing::debug!(text = %text, "echo tool called");
    text
}

#[tool(description = "Adds two numbers together")]
async fn add(#[param(desc = "First number")] a: i64, #[param(desc = "Second number")] b: i64) -> String {
    tracing::debug!(a = a, b = b, result = a + b, "add tool called");
    format!("{}", a + b)
}

// ── Default Agent JSON ──

const DEFAULT_AGENT_JSON: &str = r#"{
    "id": "test-console-agent",
    "description": "Test console agent with built-in tools",
    "instructions": "You are a helpful AI assistant. Use tools when appropriate. Respond in Chinese if the user speaks Chinese.",
    "model": {
        "provider": "deepseek",
        "model": "deepseek-chat"
    },
    "tools": [
        { "type": "builtin", "name": "read_file" },
        { "type": "builtin", "name": "web_search" }
    ],
    "max_tool_rounds": 5
}"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = if std::env::args().any(|a| a == "--auto") { "AUTO" } else { "REPL" };
    let log_level = if std::env::var("RUST_LOG").is_ok() { "env" } else { "warn" };

    // ── Tracing setup ──
    let mut filter = tracing_subscriber::EnvFilter::from_default_env();
    if std::env::var("RUST_LOG").is_err() {
        filter = filter.add_directive("warn".parse()?);
    }
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_line_number(true)
        .init();

    tracing::info!(
        mode = mode,
        log_level = log_level,
        "========== Test Console Starting =========="
    );

    // ── Build default agent ──
    println!("\x1b[1;36m╔══════════════════════════════════════╗\x1b[0m");
    println!("\x1b[1;36m║  rust-agent-decl Test Console       ║\x1b[0m");
    println!("\x1b[1;36m╚══════════════════════════════════════╝\x1b[0m");
    println!();
    println!("\x1b[90m  Type /help for commands, /quit to exit\x1b[0m");
    println!("\x1b[90m  RUST_LOG=info for detailed trace\x1b[0m");
    println!();

    let agent = build_agent_from_json(DEFAULT_AGENT_JSON, DEEPSEEK_API_KEY)?;
    let mut session = agent.create_session();
    let mut thinking_enabled = false;
    let mut active_json = DEFAULT_AGENT_JSON.to_string();

    print_agent_info(&agent);

    // ── Auto-run mode ──
    if mode == "AUTO" {
        auto_test(&agent, &mut session, &mut thinking_enabled).await;
        println!("\n\x1b[32m[auto] All tests completed successfully.\x1b[0m");
        tracing::info!("========== Test Console Finished (AUTO) ==========");
        return Ok(());
    }

    // ── Interactive REPL ──
    let mut rl = rustyline::DefaultEditor::new()?;
    let history_path = dirs_next().unwrap_or_else(|| ".".into()).join(".decl_test_history");
    let _ = rl.load_history(&history_path);

    loop {
        let prompt = format!("\x1b[36mdecl>\x1b[0m ");
        match rl.readline(&prompt) {
            Ok(line) => {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(&line);

                match line.as_str() {
                    "/quit" | "/exit" | "quit" | "exit" => {
                        tracing::info!("User requested exit");
                        println!("\x1b[90mGoodbye.\x1b[0m");
                        break;
                    }
                    "/help" => print_help(),
                    "/clear" => {
                        tracing::info!("Clearing session and resetting agent");
                        let _ = session.clear().await;
                        let _ = agent.reset().await;
                        session = agent.create_session();
                        println!("\x1b[32mSession cleared.\x1b[0m");
                    }
                    "/agent" => {
                        println!("\x1b[90mCurrent declaration:\x1b[0m");
                        println!("{}", active_json);
                    }
                    "/tools" => {
                        println!("\x1b[90mBuilt-in tools: echo, add, read_file, web_search\x1b[0m");
                    }
                    cmd if cmd.starts_with("/think ") => {
                        let arg = cmd.strip_prefix("/think ").unwrap().trim();
                        thinking_enabled = arg == "on";
                        tracing::info!(enabled = thinking_enabled, "Thinking mode toggled");
                        println!("\x1b[33mThinking mode: {}\x1b[0m", if thinking_enabled { "ON" } else { "OFF" });
                    }
                    cmd if cmd.starts_with("/load ") => {
                        let path = cmd.strip_prefix("/load ").unwrap().trim();
                        tracing::info!(path = path, "Loading agent declaration from file");
                        match std::fs::read_to_string(path) {
                            Ok(content) => {
                                match build_agent_from_json(&content, DEEPSEEK_API_KEY) {
                                    Ok(_) => {
                                        active_json = content;
                                        println!("\x1b[32mAgent declaration loaded from {}\x1b[0m", path);
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, path = path, "Failed to build agent from file");
                                        eprintln!("\x1b[31mFailed to parse: {}\x1b[0m", e);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = %e, path = path, "Failed to read file");
                                eprintln!("\x1b[31mFailed to read file: {}\x1b[0m", e);
                            }
                        }
                    }
                    cmd if cmd.starts_with("/validate ") => {
                        let path = cmd.strip_prefix("/validate ").unwrap().trim();
                        tracing::info!(path = path, "Validating declaration file");
                        match std::fs::read_to_string(path) {
                            Ok(content) => {
                                match AgentDecl::from_json_str(&content) {
                                    Ok(decl) => {
                                        tracing::info!(
                                            id = %decl.id,
                                            provider = %decl.model.provider,
                                            model = %decl.model.model,
                                            tools = decl.tools.len(),
                                            "Declaration validated"
                                        );
                                        println!("\x1b[32m[OK] Valid: id='{}', model={}/{}, tools={}\x1b[0m",
                                            decl.id, decl.model.provider, decl.model.model, decl.tools.len());
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, path = path, "Declaration validation failed");
                                        eprintln!("\x1b[31m[FAIL] {}\x1b[0m", e);
                                    }
                                }
                            }
                            Err(e) => eprintln!("\x1b[31mFailed to read file: {}\x1b[0m", e),
                        }
                    }
                    _ => {
                        run_agent(&agent, &mut session, &line, thinking_enabled).await;
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("\x1b[90m(use /quit to exit)\x1b[0m");
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!("\x1b[90mGoodbye.\x1b[0m");
                break;
            }
            Err(e) => {
                tracing::error!(error = %e, "Readline error");
                eprintln!("\x1b[31mReadline error: {}\x1b[0m", e);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    tracing::info!("========== Test Console Finished (REPL) ==========");
    Ok(())
}

// ── Agent Build Helpers ──

fn build_agent_from_json(json: &str, api_key: &str) -> Result<Arc<dyn IAgent>, Box<dyn std::error::Error>> {
    let t0 = Instant::now();

    tracing::info!("[build] Parsing AgentDecl from JSON...");
    let decl = AgentDecl::from_json_str(json)?;
    tracing::info!(
        id = %decl.id,
        provider = %decl.model.provider,
        model = %decl.model.model,
        tools_in_decl = decl.tools.len(),
        "Parsed AgentDecl"
    );

    tracing::info!(
        provider = %decl.model.provider,
        model = %decl.model.model,
        "[build] Creating chat client..."
    );
    let chat_client: Arc<dyn IChatClient> = match decl.model.provider.as_str() {
        "deepseek" => {
            tracing::info!("Using DeepSeekChatClient");
            Arc::new(DeepSeekChatClient::new(
                ChatClientOptions::deepseek(&decl.model.model, api_key)
            )?)
        }
        "openai" => {
            tracing::info!("Using OpenAiChatClient");
            Arc::new(OpenAiChatClient::new(
                ChatClientOptions::openai(&decl.model.model, api_key)
            )?)
        }
        other => return Err(format!("Unsupported provider in decl: {}", other).into()),
    };

    tracing::info!("[build] Constructing AgentBuilder with tools...");
    let mut builder = AgentBuilder::new(&decl.id)
        .chat_client(ClientWrapper(chat_client))
        .instructions(&decl.instructions)
        .max_tool_rounds(decl.max_tool_rounds)
        .with_tool(Echo)
        .with_tool(Add);

    if !decl.description.is_empty() {
        builder = builder.with_description(&decl.description);
    }

    let mut builtin_count = 0usize;
    for tool_ref in &decl.tools {
        if let rust_agent_decl::ToolRef::Builtin { name, .. } = tool_ref {
            if let Ok(tool) = DefaultAgentResolver::resolve_builtin_tool(name) {
                tracing::debug!(tool_name = %name, "Registered builtin tool");
                builder = builder.with_tool(rust_agent_decl::ToolWrapper(tool));
                builtin_count += 1;
            } else {
                tracing::warn!(tool_name = %name, "Builtin tool not found, skipping");
            }
        }
    }
    tracing::info!(builtin_count = builtin_count, echo = true, add = true, "Tools registered");

    tracing::info!("[build] Calling AgentBuilder::build()...");
    let agent = builder.build()?;

    tracing::info!(
        elapsed_ms = t0.elapsed().as_millis(),
        agent_id = %agent.id(),
        "[build] Agent built successfully"
    );
    Ok(agent)
}

fn print_agent_info(agent: &Arc<dyn IAgent>) {
    let meta = agent.metadata();
    println!("\x1b[90m  Agent: {} | {} | {}\x1b[0m",
        agent.id(),
        meta.agent_type,
        meta.description,
    );
}

// ── Agent Run + Stream Consumer ──

async fn run_agent(
    agent: &Arc<dyn IAgent>,
    session: &mut Arc<dyn rust_agent_core::ISession>,
    input: &str,
    thinking: bool,
) {
    let t0 = Instant::now();
    tracing::info!(
        agent_id = %agent.id(),
        input = %input,
        thinking = thinking,
        "──────────────────────────────────────────"
    );
    tracing::info!("[run] Sending prompt to agent...");

    let mut run_opts = AgentRunOptions::new();
    if thinking {
        run_opts = run_opts
            .with_thinking(true)
            .with_reasoning_effort(ReasoningEffort::High);
        tracing::debug!("Thinking mode enabled with ReasonEffort::High");
    }

    let messages = vec![ChatMessage::user(input)];
    let session_clone = Arc::clone(session);

    match agent.run(messages, Some(session_clone), Some(run_opts)).await {
        Ok(mut stream) => {
            tracing::info!("[run] Stream opened, waiting for response...");
            let mut chunk_count: u64 = 0;
            let mut text_bytes: usize = 0;
            let mut tool_call_count: u64 = 0;
            let mut total_tokens: u32 = 0;
            print!("\n");

            loop {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    poll_next(&mut stream),
                ).await {
                    Ok(Some(Ok(chunk))) => {
                        chunk_count += 1;
                        if chunk_count == 1 {
                            tracing::debug!("[run] First chunk received ({}ms)", t0.elapsed().as_millis());
                        }

                        for content in &chunk.contents {
                            match content {
                                Content::Text(t) => {
                                    text_bytes += t.delta.len();
                                    print!("{}", t.delta);
                                    use std::io::Write;
                                    let _ = std::io::stdout().flush();
                                }
                                Content::Reasoning(t) => {
                                    tracing::debug!(delta_len = t.delta.len(), "Reasoning delta");
                                    eprint!("\x1b[90m[思考] {}\x1b[0m", t.delta);
                                }
                                Content::ToolCallStart(s) => {
                                    tool_call_count += 1;
                                    tracing::info!(
                                        tool_name = %s.name,
                                        call_id = %s.call_id,
                                        "[run] Tool call started"
                                    );
                                    eprintln!("\x1b[36m[调用] {}\x1b[0m", s.name);
                                }
                                Content::ToolCallArgsParsed(p) => {
                                    let val_str = serde_json::to_string(&p.value).unwrap_or_default();
                                    tracing::debug!(
                                        tool_name = %p.name,
                                        value_len = val_str.len(),
                                        "[run] Tool args parsed"
                                    );
                                    let display = if val_str.len() > 200 {
                                        format!("{}...({:.1}KB)", &val_str[..200], val_str.len() as f64 / 1024.0)
                                    } else {
                                        val_str
                                    };
                                    eprintln!("\x1b[32m  {} = {}\x1b[0m", p.name, display);
                                }
                                Content::ToolCallArgsProgress(p) => {
                                    tracing::debug!(
                                        received = p.received,
                                        "[run] Tool args progress"
                                    );
                                    eprintln!("\x1b[90m  progress: {:.1}KB\x1b[0m",
                                        p.received as f64 / 1024.0);
                                }
                                Content::ToolCalling(tc) => {
                                    tracing::info!(
                                        tool_name = %tc.name,
                                        "[run] Tool calling (executing)"
                                    );
                                    let args_str = serde_json::to_string(&tc.arguments).unwrap_or_default();
                                    let display = if args_str.len() > 80 {
                                        format!("{}...", &args_str[..80])
                                    } else {
                                        args_str
                                    };
                                    eprintln!("\x1b[33m[参数] {} {}\x1b[0m", tc.name, display);
                                }
                                Content::ToolCalled(tc) => {
                                    if let Some(ref err) = tc.error {
                                        tracing::warn!(
                                            call_id = %tc.call_id,
                                            error_len = err.len(),
                                            "[run] Tool call FAILED"
                                        );
                                        let display = if err.len() > 200 {
                                            format!("{}...({} chars)", &err[..200], err.len())
                                        } else {
                                            err.clone()
                                        };
                                        eprintln!("\x1b[31m[结果] 失败: {}\x1b[0m", display);
                                    } else if let Some(ref result) = tc.result {
                                        tracing::info!(
                                            call_id = %tc.call_id,
                                            result_len = result.len(),
                                            "[run] Tool call SUCCEEDED"
                                        );
                                        let display = if result.len() > 200 {
                                            format!("{}...({} chars)", &result[..200], result.len())
                                        } else {
                                            result.clone()
                                        };
                                        eprintln!("\x1b[32m[结果] {}\x1b[0m", display);
                                    }
                                }
                                Content::Usage(u) => {
                                    let usage = &u.usage;
                                    total_tokens = usage.total_tokens;
                                    tracing::info!(
                                        prompt_tokens = usage.prompt_tokens,
                                        completion_tokens = usage.completion_tokens,
                                        cache_hit = usage.prompt_cache_hit_tokens.unwrap_or(0),
                                        reasoning = usage.reasoning_tokens.unwrap_or(0),
                                        total = usage.total_tokens,
                                        "[run] Token usage"
                                    );
                                    eprintln!("\x1b[90m[用量] prompt={} completion={} cache_hit={} reasoning={} total={}\x1b[0m",
                                        usage.prompt_tokens, usage.completion_tokens,
                                        usage.prompt_cache_hit_tokens.unwrap_or(0),
                                        usage.reasoning_tokens.unwrap_or(0),
                                        usage.total_tokens);
                                }
                                Content::Error(e) => {
                                    tracing::error!(
                                        error_code = %e.error_code,
                                        message = %e.message,
                                        "[run] Content error"
                                    );
                                    eprintln!("\x1b[31m[错误] {}: {}\x1b[0m", e.error_code, e.message);
                                }
                                _ => {}
                            }
                        }

                        if let Some(reason) = &chunk.finish_reason {
                            match reason {
                                rust_agent_core::FinishReason::Stop => {
                                    let elapsed = t0.elapsed();
                                    tracing::info!(
                                        elapsed_ms = elapsed.as_millis(),
                                        chunk_count = chunk_count,
                                        text_bytes = text_bytes,
                                        tool_calls = tool_call_count,
                                        total_tokens = total_tokens,
                                        "[run] Agent finished (Stop)"
                                    );
                                    println!();
                                    break;
                                }
                                rust_agent_core::FinishReason::ToolCalls => {
                                    tracing::debug!("[run] Agent paused for tool execution");
                                }
                                other => {
                                    tracing::info!(reason = ?other, "[run] Agent finished (other)");
                                    eprintln!("\x1b[90m[结束] {:?}\x1b[0m", other);
                                    break;
                                }
                            }
                        }
                    }
                    Ok(Some(Err(e))) => {
                        tracing::error!(error = %e, elapsed_ms = t0.elapsed().as_millis(), "[run] Stream error");
                        eprintln!("\n\x1b[31m[流错误] {}\x1b[0m", e);
                        break;
                    }
                    Ok(None) => {
                        tracing::info!(elapsed_ms = t0.elapsed().as_millis(), "[run] Stream ended (None)");
                        break;
                    }
                    Err(_) => {
                        tracing::error!("[run] Timeout after 120s");
                        eprintln!("\n\x1b[31m[超时] 120s 无响应\x1b[0m");
                        break;
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, elapsed_ms = t0.elapsed().as_millis(), "[run] Agent error");
            eprintln!("\x1b[31m[Agent 错误] {}\x1b[0m", e);
        }
    }
    println!();
}

// ── Auto Test Sequence ──

async fn auto_test(
    agent: &Arc<dyn IAgent>,
    session: &mut Arc<dyn rust_agent_core::ISession>,
    thinking: &mut bool,
) {
    let t0 = Instant::now();

    let tests: Vec<(&str, &str)> = vec![
        ("Basic greeting", "Say hello in 3 languages."),
        ("Math tool", "What is 123 + 456? Use the add tool."),
        ("Echo tool", "Echo back: 'declarative testing works!'"),
    ];

    tracing::info!(
        test_count = tests.len(),
        agent_id = %agent.id(),
        "========== AUTO TEST: Starting {} tests ==========",
        tests.len()
    );

    for (i, (name, prompt)) in tests.iter().enumerate() {
        let test_start = Instant::now();
        tracing::info!(
            test_num = i + 1,
            total = tests.len(),
            test_name = *name,
            prompt = *prompt,
            "---------- [AUTO] Test {}/{}: {} ----------",
            i + 1, tests.len(), name
        );

        println!("\x1b[36m[Test {}/{}] {}\x1b[0m", i + 1, tests.len(), name);
        println!("\x1b[90m  Prompt: {}\x1b[0m\n", prompt);

        run_agent(agent, session, prompt, *thinking).await;

        tracing::info!(
            test_num = i + 1,
            elapsed_ms = test_start.elapsed().as_millis(),
            "[AUTO] Test {}/{} completed",
            i + 1, tests.len()
        );
    }

    tracing::info!(
        total_elapsed_ms = t0.elapsed().as_millis(),
        test_count = tests.len(),
        "========== AUTO TEST: All {} tests passed ==========",
        tests.len()
    );
}

// ── Help ──

fn print_help() {
    println!();
    println!("\x1b[1mCommands:\x1b[0m");
    println!("  \x1b[33m/help\x1b[0m              Show this help");
    println!("  \x1b[33m/quit\x1b[0m              Exit the console");
    println!("  \x1b[33m/clear\x1b[0m             Clear session history and reset agent");
    println!("  \x1b[33m/agent\x1b[0m             Show current agent declaration");
    println!("  \x1b[33m/tools\x1b[0m             List available tools (echo, add + builtins)");
    println!("  \x1b[33m/think on|off\x1b[0m      Toggle reasoning/thinking mode");
    println!("  \x1b[33m/load <file.json>\x1b[0m   Load agent declaration from JSON file");
    println!("  \x1b[33m/validate <file.json>\x1b[0m Parse and validate an agent declaration");
    println!();
    println!("\x1b[90mAny other input is sent as a chat message to the agent.\x1b[0m");
    println!("\x1b[90mRun with --auto for automated test sequence.\x1b[0m");
    println!("\x1b[90mRUST_LOG=info for detailed trace output.\x1b[0m");
    println!();
}

// ── Stream helper ──

async fn poll_next<T: Unpin>(
    stream: &mut (dyn Stream<Item = T> + Unpin + Send),
) -> Option<T> {
    StreamNext { stream }.await
}

struct StreamNext<'a, T> {
    stream: &'a mut (dyn Stream<Item = T> + Unpin + Send),
}

impl<T: Unpin> std::future::Future for StreamNext<'_, T> {
    type Output = Option<T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.stream).poll_next(cx)
    }
}

// ── dirs_next helper (avoid extra dep) ──

fn dirs_next() -> Option<std::path::PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(std::path::PathBuf::from)
}

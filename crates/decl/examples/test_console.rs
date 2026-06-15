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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("warn".parse()?)
        )
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
                                Content::ToolCall
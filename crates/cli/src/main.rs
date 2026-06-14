use std::sync::Arc;

use futures_util::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_core::{
    AgentRunOptions, AgentSession, ChatMessage, Content, ISession,
};
use rust_agent_framework::tool;
use rust_agent_framework::AgentBuilder;

// ── Hardcoded API key for development ──────────────────────────
const DEEPSEEK_API_KEY: &str = "sk-6eab5986594445abab4dfd0bd2957ee";

// ── Tool definitions ───────────────────────────────────────────
#[tool(description = "Echoes back the input text")]
async fn echo(#[param(desc = "The text to echo")] text: String) -> String {
    text
}

#[tool(description = "Adds two numbers together")]
async fn add(#[param(desc = "First number")] a: i64, #[param(desc = "Second number")] b: i64) -> String {
    format!("{}", a + b)
}

// ── Commands ───────────────────────────────────────────────────
fn print_help() {
    println!("Commands:");
    println!("  /help        Show this help");
    println!("  /clear       Clear conversation history");
    println!("  /think on    Enable thinking mode");
    println!("  /think off   Disable thinking mode");
    println!("  /model NAME  Switch model (e.g. deepseek-chat, deepseek-reasoner)");
    println!("  /quit|exit   Exit (also: quit, exit without slash)");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Minimal logging — only warnings and above to keep chat output clean
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("warn".parse()?)
        )
        .init();

    // ── Build client & agent ───────────────────────────────────
    let options = ChatClientOptions::deepseek("deepseek-v4-flash", DEEPSEEK_API_KEY);
    let client = DeepSeekChatClient::new(options)?;

    let mut agent = AgentBuilder::new("cli-agent")
        .chat_client(client)
        .instructions("You are a helpful AI assistant. Respond concisely.")
        .with_tool(Echo)
        .with_tool(Add)
        .build()?;

    let session = Arc::new(AgentSession::new());

    let mut thinking_enabled = true;

    // ── REPL ───────────────────────────────────────────────────
    println!("rust-agent-cli — Interactive Chat (DeepSeek)");
    println!("Type /help for commands, /quit to exit.\n");

    let mut rl = DefaultEditor::new()?;
    let prompt = "> ".to_string();

    loop {
        let line = rl.readline(&prompt);
        match line {
            Ok(input) => {
                let trimmed = input.trim();

                if trimmed.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(trimmed);

                // ── Handle commands ────────────────────────────
                if trimmed == "/quit" || trimmed == "/exit" || trimmed == "quit" || trimmed == "exit" {
                    println!("Bye!");
                    break;
                }
                if trimmed == "/help" {
                    print_help();
                    continue;
                }
                if trimmed == "/clear" {
                    session.clear().await?;
                    agent.reset().await?;
                    println!("[History cleared]\n");
                    continue;
                }
                if let Some(arg) = trimmed.strip_prefix("/think") {
                    match arg.trim() {
                        "on" => {
                            thinking_enabled = true;
                            println!("[Thinking mode enabled]\n");
                        }
                        "off" => {
                            thinking_enabled = false;
                            println!("[Thinking mode disabled]\n");
                        }
                        _ => println!("Usage: /think on|off\n"),
                    }
                    continue;
                }
                if let Some(arg) = trimmed.strip_prefix("/model") {
                    let model = arg.trim();
                    if model.is_empty() {
                        println!("Usage: /model <name>\n");
                    } else {
                        let opts = ChatClientOptions::deepseek(model, DEEPSEEK_API_KEY);
                        match DeepSeekChatClient::new(opts) {
                            Ok(new_client) => {
                                agent = AgentBuilder::new("cli-agent")
                                    .chat_client(new_client)
                                    .instructions("You are a helpful AI assistant. Respond concisely.")
                                    .build()?;
                                println!("[Model switched to {}]\n", model);
                            }
                            Err(e) => println!("[Error creating client: {}]\n", e),
                        }
                    }
                    continue;
                }

                // ── Chat ──────────────────────────────────────
                let messages = vec![ChatMessage::user(trimmed)];
                let mut run_opts = AgentRunOptions::new();
                if thinking_enabled {
                    run_opts = run_opts
                        .with_thinking(true)
                        .with_reasoning_effort(rust_agent_core::ReasoningEffort::High);
                }

                let result = agent.run(messages, Some(session.clone()), Some(run_opts)).await;
                match result {
                    Ok(mut stream) => {
                        let mut in_reasoning = false;

                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(chunk) => {
                                    for content in &chunk.contents {
                                        match content {
                                            Content::Text(c) => {
                                                if in_reasoning {
                                                    print!("\x1b[0m");
                                                    in_reasoning = false;
                                                }
                                                print!("{}", c.delta);
                                            }
                                            Content::Reasoning(c) => {
                                                if !in_reasoning {
                                                    print!("\x1b[90m[思考] ");
                                                    in_reasoning = true;
                                                }
                                                print!("{}", c.delta);
                                            }
                                            Content::ToolCalling(c) => {
                                                if in_reasoning {
                                                    print!("\x1b[0m");
                                                    in_reasoning = false;
                                                }
                                                eprintln!("\n[工具调用] {} args={}", c.name, c.arguments);
                                            }
                                            Content::ToolCalled(c) => {
                                                if let Some(err) = &c.error {
                                                    eprintln!("[工具错误] {}", err);
                                                } else {
                                                    eprintln!("[工具结果] {}", c.result.as_deref().unwrap_or(""));
                                                }
                                            }
                                            Content::Usage(c) => {
                                                let hit = c.usage.prompt_cache_hit_tokens.unwrap_or(0);
                                                let miss = c.usage.prompt_cache_miss_tokens.unwrap_or(0);
                                                if hit > 0 || miss > 0 {
                                                    eprintln!("\n[缓存] 命中{}/{} tokens", hit, hit + miss);
                                                }
                                                eprintln!(
                                                    "[用量] prompt={} completion={} total={}",
                                                    c.usage.prompt_tokens,
                                                    c.usage.completion_tokens,
                                                    c.usage.total_tokens
                                                );
                                            }
                                            Content::Error(c) => {
                                                if in_reasoning {
                                                    print!("\x1b[0m");
                                                    in_reasoning = false;
                                                }
                                                eprintln!("\n[错误] {}: {}", c.error_code, c.message);
                                            }
                                            _ => {}
                                        }
                                    }

                                    if let Some(reason) = &chunk.finish_reason {
                                        if in_reasoning {
                                            print!("\x1b[0m");
                                            in_reasoning = false;
                                        }
                                        eprintln!("\n[结束] {:?}", reason);
                                    }

                                    for event in &chunk.events {
                                        match event {
                                            rust_agent_core::Event::ExecutorInvoking(e) => {
                                                eprintln!("[调度] {} 开始", e.executor_id);
                                            }
                                            rust_agent_core::Event::ExecutorInvoked(e) => {
                                                eprintln!(
                                                    "[调度] {} 完成 {}ms",
                                                    e.executor_id, e.duration_ms
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Err(e) => {
                                    if in_reasoning {
                                        print!("\x1b[0m");
                                        in_reasoning = false;
                                    }
                                    eprintln!("\n[错误] {}", e);
                                }
                            }
                        }
                        if in_reasoning {
                            print!("\x1b[0m");
                        }
                        println!("\n");
                    }
                    Err(e) => {
                        eprintln!("[错误] {}", e);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("(Ctrl+C — type /quit to exit)\n");
            }
            Err(ReadlineError::Eof) => {
                println!("Bye!");
                break;
            }
            Err(err) => {
                eprintln!("Read error: {}", err);
                break;
            }
        }
    }

    Ok(())
}

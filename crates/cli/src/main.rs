use std::io::Write;
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
const DEEPSEEK_API_KEY: &str = "sk-9f8dbaaa822e477faf339e32cdb89e91";

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

                #[cfg(debug_assertions)]
                {
                    // Dump session state before the call to validate message history
                    let history = session.get_messages().await.unwrap_or_default();
                    eprintln!("\x1b[90m[调试] session中已有{}条消息\x1b[0m", history.len());
                    for (i, m) in history.iter().enumerate() {
                        let role = match m.role {
                            rust_agent_core::MessageRole::System => "system",
                            rust_agent_core::MessageRole::User => "user",
                            rust_agent_core::MessageRole::Assistant => {
                                if m.tool_calls.is_some() { "assistant+tool_calls" } else { "assistant" }
                            }
                            rust_agent_core::MessageRole::Tool => "tool",
                        };
                        let content_preview = if m.content.len() > 60 {
                            format!("{}...", &m.content[..57])
                        } else {
                            m.content.clone()
                        };
                        eprintln!("\x1b[90m  [{i}] {role}: {content_preview}\x1b[0m");
                    }
                }
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
                        // Track active parallel tool calls for friendly progress display
                        let mut active_tools: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();

                        while let Some(item) = stream.next().await {
                            match item {
                                Ok(chunk) => {
                                    for content in &chunk.contents {
                                        match content {
                                            Content::Text(c) => {
                                                if in_reasoning {
                                                    println!("\x1b[0m");
                                                    in_reasoning = false;
                                                }
                                                print!("{}", c.delta);
                                                std::io::stdout().flush().unwrap();
                                            }
                                            Content::Reasoning(c) => {
                                                if !in_reasoning {
                                                    print!("\x1b[90m[思考] ");
                                                    in_reasoning = true;
                                                }
                                                print!("{}", c.delta);
                                                std::io::stdout().flush().unwrap();
                                            }
                                            // ── Streaming tool call lifecycle ──
                                            Content::ToolCallStart(c) => {
                                                if in_reasoning {
                                                    println!("\x1b[0m");
                                                    in_reasoning = false;
                                                }
                                                let label = format!(
                                                    "\x1b[36m[调用] {}\x1b[0m",
                                                    c.name
                                                );
                                                eprint!("\n{} 接收参数中", label);
                                                active_tools.insert(c.call_id.clone(), label);
                                            }
                                            Content::ToolCallArgs(_c) => {
                                                // Args deltas arrive during streaming — show
                                                // progress dots. Skip if no active tool (should
                                                // not happen, but guard).
                                                if !active_tools.is_empty() {
                                                    eprint!(".");
                                                    std::io::stderr().flush().unwrap();
                                                }
                                            }
                                            Content::ToolCallEnd(_c) => {
                                                // Args complete — shown by ToolCalling below
                                                let _ = active_tools.remove(&_c.call_id);
                                            }
                                            // ── Complete tool calls ──
                                            Content::ToolCalling(c) => {
                                                if in_reasoning {
                                                    println!("\x1b[0m");
                                                    in_reasoning = false;
                                                }
                                                // Show compact one-line summary
                                                let args_str = c.arguments.to_string();
                                                let args_preview = if args_str.len() > 80 {
                                                    format!("{}...", &args_str[..77])
                                                } else {
                                                    args_str
                                                };
                                                eprintln!(
                                                    "\n\x1b[33m[参数] {}\x1b[0m {}",
                                                    c.name, args_preview
                                                );
                                            }
                                            Content::ToolCalled(c) => {
                                                if let Some(err) = &c.error {
                                                    eprintln!("\x1b[31m[结果] 失败\x1b[0m {}", err);
                                                } else {
                                                    let r = c.result.as_deref().unwrap_or("");
                                                    let preview = if r.len() > 100 {
                                                        format!("{}...", &r[..97])
                                                    } else {
                                                        r.to_string()
                                                    };
                                                    eprintln!("\x1b[32m[结果]\x1b[0m {}", preview);
                                                }
                                            }
                                            Content::Usage(c) => {
                                                let cache = if c.usage.prompt_cache_hit_tokens.unwrap_or(0) > 0 {
                                                    let ratio = c.usage.cache_hit_ratio();
                                                    format!(" 缓存命中{:.0}%", ratio * 100.0)
                                                } else {
                                                    String::new()
                                                };
                                                eprintln!(
                                                    "\n\x1b[90m[用量] prompt={} completion={} total={}{}\x1b[0m",
                                                    c.usage.prompt_tokens,
                                                    c.usage.completion_tokens,
                                                    c.usage.total_tokens,
                                                    cache,
                                                );
                                            }
                                            Content::Error(c) => {
                                                if in_reasoning {
                                                    println!("\x1b[0m");
                                                    in_reasoning = false;
                                                }
                                                eprintln!("\n\x1b[31m[错误]\x1b[0m {}: {}", c.error_code, c.message);
                                            }
                                            _ => {}
                                        }
                                    }

                                    if let Some(reason) = &chunk.finish_reason {
                                        if in_reasoning {
                                            println!("\x1b[0m");
                                            in_reasoning = false;
                                        }
                                        match reason {
                                            rust_agent_core::FinishReason::Stop => {
                                                // Normal end — no extra output
                                            }
                                            rust_agent_core::FinishReason::ToolCalls => {
                                                // Tool calls follow — shown above
                                            }
                                            _ => {
                                                eprintln!("\n\x1b[90m[结束] {:?}\x1b[0m", reason);
                                            }
                                        }
                                    }

                                    for event in &chunk.events {
                                        match event {
                                            rust_agent_core::Event::ExecutorInvoking(e) => {
                                                eprintln!("\x1b[90m[调度] {} 开始\x1b[0m", e.executor_id);
                                            }
                                            rust_agent_core::Event::ExecutorInvoked(e) => {
                                                eprintln!(
                                                    "\x1b[90m[调度] {} 完成 {}ms\x1b[0m",
                                                    e.executor_id, e.duration_ms
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Err(e) => {
                                    if in_reasoning {
                                        println!("\x1b[0m");
                                        in_reasoning = false;
                                    }
                                    eprintln!("\n\x1b[31m[错误]\x1b[0m {}", e);
                                }
                            }
                        }
                        if in_reasoning {
                            println!("\x1b[0m");
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

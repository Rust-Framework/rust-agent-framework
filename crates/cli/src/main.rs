use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use futures_util::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_core::{
    AgentRunOptions, AgentSession, ChatMessage, Content, ISession,
};
use rust_agent_framework::memory::SkillMemoryContextProvider;
use rust_agent_framework::tool;
use rust_agent_framework::AgentBuilder;
use rust_agent_websearch::{WebSearch, WebFetch};

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
    println!("  /restart     Clear history + reset agent (simulates new session)");
    println!("  /memory      Show SkillMemory status");
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

    // ── Resolve SkillMemory directory ─────────────────────────
    let memory_dir = {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let rel = base.join("../framework/src/memory/skill");
        if rel.exists() {
            rel.canonicalize().unwrap_or(rel)
        } else {
            // Fallback for running from workspace root
            PathBuf::from("crates/framework/src/memory/skill")
        }
    };
    println!("[SkillMemory dir: {}]", memory_dir.display());

    let skill_memory = SkillMemoryContextProvider::new(&memory_dir);
    println!("[SkillMemory loaded: SKILL.md found = {}]",
        memory_dir.join("SKILL.md").exists());

    // ── Build client & agent ───────────────────────────────────
    let options = ChatClientOptions::deepseek("deepseek-v4-flash", DEEPSEEK_API_KEY);
    let client = DeepSeekChatClient::new(options)?;

    let mut agent = AgentBuilder::new("cli-agent")
        .chat_client(client)
        .instructions("You are a helpful AI assistant. Respond concisely.")
        .with_tool(Echo)
        .with_tool(Add)
        .with_tool(WebSearch)
        .with_tool(WebFetch)
        .add_context_provider(skill_memory)
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
                if trimmed == "/restart" {
                    session.clear().await?;
                    agent.reset().await?;
                    // Rebuild agent to force SkillMemory re-initialization
                    let opts = ChatClientOptions::deepseek("deepseek-v4-flash", DEEPSEEK_API_KEY);
                    let new_client = DeepSeekChatClient::new(opts)?;
                    let skill_memory_new = SkillMemoryContextProvider::new(&memory_dir);
                    agent = AgentBuilder::new("cli-agent")
                        .chat_client(new_client)
                        .instructions("You are a helpful AI assistant. Respond concisely.")
                        .with_tool(Echo)
                        .with_tool(Add)
                        .with_tool(WebSearch)
                        .with_tool(WebFetch)
                        .add_context_provider(skill_memory_new)
                        .build()?;
                    println!("[Session restarted — history cleared, agent reset]\n");
                    continue;
                }
                if trimmed == "/memory" {
                    println!("[SkillMemory]");
                    println!("  dir: {}", memory_dir.display());
                    println!("  enabled: true");
                    println!("  SKILL.md: {}", memory_dir.join("SKILL.md").exists());
                    println!("  AGENT.md: {}", memory_dir.join("AGENT.md").exists());
                    println!("  references:");
                    for entry in std::fs::read_dir(memory_dir.join("references")).ok().into_iter().flatten() {
                        if let Ok(e) = entry {
                            println!("    - {}", e.file_name().to_string_lossy());
                        }
                    }
                    println!("  assets:");
                    for entry in std::fs::read_dir(memory_dir.join("assets")).ok().into_iter().flatten() {
                        if let Ok(e) = entry {
                            println!("    - {}", e.file_name().to_string_lossy());
                        }
                    }
                    println!();
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
                                                eprintln!("\n{} 参数:", label);
                                                active_tools.insert(c.call_id.clone(), label);
                                            }
                                            Content::ToolCallArgs(_c) => {
                                                // Raw deltas — progress shown via
                                                // ToolCallArgsProgress below.
                                            }
                                            Content::ToolCallArgsParsed(c) => {
                                                // A parameter value is complete — show key=value
                                                let val_str = if c.value.is_string() {
                                                    let s = c.value.as_str().unwrap_or("");
                                                    if s.len() > 60 {
                                                        format!("\"{}\" ({:.1}KB)", s, s.len() as f64 / 1024.0)
                                                    } else {
                                                        format!("\"{}\"", s)
                                                    }
                                                } else {
                                                    c.value.to_string()
                                                };
                                                eprintln!(
                                                    "  \x1b[32m{}\x1b[0m = {}",
                                                    c.name, val_str
                                                );
                                            }
                                            Content::ToolCallArgsProgress(c) => {
                                                // Live progress for a long string parameter — use \r to create typewriter effect
                                                let preview = c.value.as_str().unwrap_or("");
                                                let preview = if preview.chars().count() > 60 {
                                                    let tail: String = preview.chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
                                                    format!("{}...", tail)
                                                } else {
                                                    preview.to_string()
                                                };
                                                eprint!(
                                                    "\r  \x1b[90m{} ({:.1}KB)\x1b[0m {}  ",
                                                    c.name,
                                                    c.received as f64 / 1024.0,
                                                    preview,
                                                );
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
                                                let args_preview = if args_str.chars().count() > 80 {
                                                    let head: String = args_str.chars().take(77).collect();
                                                    format!("{}...", head)
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
                                                    let preview = if r.chars().count() > 100 {
                                                        let head: String = r.chars().take(97).collect();
                                                        format!("{}...", head)
                                                    } else {
                                                        r.to_string()
                                                    };
                                                    eprintln!("\x1b[32m[结果]\x1b[0m {}", preview);
                                                }
                                            }
                                            Content::Usage(c) => {
                                                let cache = if let Some(hit) = c.usage.prompt_cache_hit_tokens {
                                                    let miss = c.usage.prompt_cache_miss_tokens.unwrap_or(0);
                                                    let total_cache = hit + miss;
                                                    if total_cache > 0 {
                                                        let ratio = c.usage.cache_hit_ratio();
                                                        format!(" cache:{}/{}({:.0}%)", hit, total_cache, ratio * 100.0)
                                                    } else {
                                                        format!(" cache:{}h", hit)
                                                    }
                                                } else {
                                                    String::new()
                                                };
                                                let reasoning = if let Some(rt) = c.usage.reasoning_tokens {
                                                    if rt > 0 { format!(" reasoning:{}", rt) } else { String::new() }
                                                } else { String::new() };
                                                eprintln!(
                                                    "\n\x1b[90m[用量] prompt={}{} completion={}{} total={}\x1b[0m",
                                                    c.usage.prompt_tokens, cache,
                                                    c.usage.completion_tokens, reasoning,
                                                    c.usage.total_tokens,
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

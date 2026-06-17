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
use rust_agent_framework::memory::scan_index_gaps;
use rust_agent_framework::tool;
use rust_agent_framework::AgentBuilder;
use rust_agent_websearch::{WebSearch, WebFetch};

// ── Hardcoded API key for development ──────────────────────────
const DEEPSEEK_API_KEY: &str = "sk-b8136a230aea467e8cdfe4649cab2d3e";

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

fn print_assets_tree(dir: &std::path::Path, indent: &str) {
    if !dir.is_dir() {
        return;
    }
    println!("{}assets:", indent.trim_end());
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut names: Vec<_> = entries.flatten().collect();
        names.sort_by_key(|e| e.file_name());
        for entry in names {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if path.is_dir() {
                println!("{}{}/", indent, name);
                print_assets_tree(&path, &format!("{}  ", indent));
            } else {
                println!("{}{}", indent, name);
            }
        }
    }
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
    // Runtime memory lives under logs/memory/ — never write into the
    // source tree.  SkillMemoryContextProvider seeds it from the
    // built-in template on first run.
    let memory_dir = PathBuf::from("logs/memory");
    std::fs::create_dir_all(&memory_dir).ok();
    println!("[SkillMemory dir: {}]", memory_dir.canonicalize().unwrap_or_else(|_| memory_dir.clone()).display());

    let skill_memory = Arc::new(
        SkillMemoryContextProvider::new(&memory_dir).with_consolidation_interval(1),
    ); // 开发环境：每轮都触发，便于测试验证
    println!("[SkillMemory loaded: SKILL.md found = {}]",
        memory_dir.join("SKILL.md").exists());

    // ── Build client & agent ───────────────────────────────────
    let options = ChatClientOptions::deepseek("deepseek-v4-flash", DEEPSEEK_API_KEY);
    let client = DeepSeekChatClient::new(options)?;

    let mut agent = AgentBuilder::new("cli-agent")
        .chat_client(client)
        .instructions(
            "You are a helpful AI assistant. Respond concisely.\n\n\
             **Behavior rules:**\n\
             1. Never mention internal system components (memory system, agents, tools, pipelines) in your responses. The user sees you as a single coherent assistant, not a collection of subsystems.\n\
             2. When you use tools or access memory, do not describe the mechanism. Simply use them and present the result naturally.\n\
             3. Do not explain your thought process step-by-step unless the user explicitly asks you to think aloud.\n\
             4. If you don't know something, admit it directly without commenting on what tools or data sources are available to you.\n\n\
             **Memory-first principle:**\n\
             5. Your identity, the user's identity, and your shared purpose are in persistent memory. For identity questions, retrieve from memory before answering — never use training-data defaults.\n\n\
             **Context reuse principle:**\n\
             6. Tool call results from earlier turns are already in conversation history. Read each file once per conversation — never re-read files you've already accessed."
        )
        .with_tool(Echo)
        .with_tool(Add)
        .with_tool(WebSearch)
        .with_tool(WebFetch)
        .add_context_provider_shared(Arc::clone(&skill_memory) as Arc<dyn rust_agent_core::IContextProvider>)
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
                    // Rebuild agent (reuse same SkillMemory provider + worker)
                    let opts = ChatClientOptions::deepseek("deepseek-v4-flash", DEEPSEEK_API_KEY);
                    let new_client = DeepSeekChatClient::new(opts)?;
                    agent = AgentBuilder::new("cli-agent")
                        .chat_client(new_client)
                        .instructions(
            "You are a helpful AI assistant. Respond concisely.\n\n\
             **Behavior rules:**\n\
             1. Never mention internal system components (memory system, agents, tools, pipelines) in your responses. The user sees you as a single coherent assistant, not a collection of subsystems.\n\
             2. When you use tools or access memory, do not describe the mechanism. Simply use them and present the result naturally.\n\
             3. Do not explain your thought process step-by-step unless the user explicitly asks you to think aloud.\n\
             4. If you don't know something, admit it directly without commenting on what tools or data sources are available to you.\n\n\
             **Memory-first principle:**\n\
             5. Your identity, the user's identity, and your shared purpose are in persistent memory. For identity questions, retrieve from memory before answering — never use training-data defaults.\n\n\
             **Context reuse principle:**\n\
             6. Tool call results from earlier turns are already in conversation history. Read each file once per conversation — never re-read files you've already accessed."
        )
                        .with_tool(Echo)
                        .with_tool(Add)
                        .with_tool(WebSearch)
                        .with_tool(WebFetch)
                        .add_context_provider_shared(Arc::clone(&skill_memory) as Arc<dyn rust_agent_core::IContextProvider>)
                        .build()?;
                    println!("[Session restarted — history cleared, agent reset]\n");
                    continue;
                }
                if trimmed == "/memory" {
                    let stats = skill_memory.worker_stats();
                    println!("[SkillMemory]");
                    println!("  dir: {}", memory_dir.display());
                    println!("  enabled: true");
                    println!("  SKILL.md: {}", memory_dir.join("SKILL.md").exists());
                    println!("  AGENT.md: {}", memory_dir.join("AGENT.md").exists());
                    println!("  worker running: {}", stats.running);
                    println!("  worker pending: {}", stats.pending);
                    println!("  worker total runs: {}", stats.total_runs);
                    println!("  worker coalesced dropped: {}", stats.total_coalesced_dropped);
                    println!("  (consolidation events: RUST_LOG=info or MEMORY_OBS_LEVEL=dev)");
                    println!("  references:");
                    for entry in std::fs::read_dir(memory_dir.join("references")).ok().into_iter().flatten() {
                        if let Ok(e) = entry {
                            println!("    - {}", e.file_name().to_string_lossy());
                        }
                    }
                    print_assets_tree(&memory_dir.join("assets"), "  ");
                    let gaps = scan_index_gaps(&memory_dir);
                    if gaps.is_empty() {
                        println!("  index: OK (no gaps)");
                    } else {
                        println!("  index gaps:");
                        for g in &gaps {
                            println!("    - {} ({})", g.path.display(), g.reason);
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
                                    .instructions(
            "You are a helpful AI assistant. Respond concisely.\n\n\
             **Behavior rules:**\n\
             1. Never mention internal system components (memory system, agents, tools, pipelines) in your responses. The user sees you as a single coherent assistant, not a collection of subsystems.\n\
             2. When you use tools or access memory, do not describe the mechanism. Simply use them and present the result naturally.\n\
             3. Do not explain your thought process step-by-step unless the user explicitly asks you to think aloud.\n\
             4. If you don't know something, admit it directly without commenting on what tools or data sources are available to you.\n\n\
             **Memory-first principle:**\n\
             5. Your identity, the user's identity, and your shared purpose are in persistent memory. For identity questions, retrieve from memory before answering — never use training-data defaults.\n\n\
             **Context reuse principle:**\n\
             6. Tool call results from earlier turns are already in conversation history. Read each file once per conversation — never re-read files you've already accessed."
        )
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
                        // Accumulate token usage for end-of-round summary
                        let mut accumulated_usage: Option<rust_agent_core::Usage> = None;

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
                                                accumulated_usage = Some(c.usage.clone());
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
                        // ── End-of-round token usage summary ──
                        if let Some(ref usage) = accumulated_usage {
                            let cache = if let Some(hit) = usage.prompt_cache_hit_tokens {
                                let miss = usage.prompt_cache_miss_tokens.unwrap_or(0);
                                let total_cache = hit + miss;
                                if total_cache > 0 {
                                    let ratio = usage.cache_hit_ratio();
                                    format!(" cache:{}/{}({:.0}%)", hit, total_cache, ratio * 100.0)
                                } else {
                                    format!(" cache:{}h", hit)
                                }
                            } else {
                                String::new()
                            };
                            let reasoning = if let Some(rt) = usage.reasoning_tokens {
                                if rt > 0 { format!(" reasoning:{}", rt) } else { String::new() }
                            } else { String::new() };
                            eprintln!(
                                "\x1b[90m[用量] prompt={}{} completion={}{} total={}\x1b[0m",
                                usage.prompt_tokens, cache,
                                usage.completion_tokens, reasoning,
                                usage.total_tokens,
                            );
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

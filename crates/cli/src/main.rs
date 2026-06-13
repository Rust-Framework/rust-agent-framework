use std::sync::Arc;

use futures_util::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use rust_agent_client::{ChatClientOptions, DeepSeekChatClient};
use rust_agent_core::{
    ChatAgentRunOptions, ChatMessage, IAgent, ISession, AgentSession,
    ReasoningEffort, ToolRegistry,
};
use rust_agent_framework::tool;
use rust_agent_framework::ChatClientAgent;

// ── Hardcoded API key for development ──────────────────────────
const DEEPSEEK_API_KEY: &str = "sk-6e2ab5986594445abab4dfd0bd2957ee";

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

    let mut tools = ToolRegistry::new();
    tools.register(Echo);
    tools.register(Add);

    let agent = ChatClientAgent::new("assistant", Arc::new(client))
        .with_instructions("You are a helpful AI assistant. Respond concisely.")
        .with_tools(tools)
        .with_description("Interactive chat agent");

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
                        // Rebuild client with new model
                        let new_options = ChatClientOptions::deepseek(model, DEEPSEEK_API_KEY);
                        match DeepSeekChatClient::new(new_options) {
                            Ok(_new_client) => {
                                // We need to rebuild the agent — for simplicity,
                                // just inform the user this needs a restart
                                println!("[Model switch requires restart. Set model at launch.]\n");
                            }
                            Err(e) => println!("[Error creating client: {}]\n", e),
                        }
                    }
                    continue;
                }

                // ── Chat ──────────────────────────────────────
                session.add_message(ChatMessage::user(trimmed)).await?;

                let messages = session.get_messages().await?;
                let mut run_opts = ChatAgentRunOptions::new();
                if thinking_enabled {
                    run_opts = run_opts
                        .with_thinking(true)
                        .with_reasoning_effort(ReasoningEffort::High);
                }

                let stream = agent.run(messages, run_opts).await?;

                // Stream output token by token
                let mut full_text = String::new();
                let mut reasoning_buf = String::new();
                let mut in_reasoning = false;

                tokio::pin!(stream);
                while let Some(chunk_result) = stream.next().await {
                    let chunk = match chunk_result {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("\n[Stream error: {}]", e);
                            break;
                        }
                    };

                    // Print reasoning (thinking) content in dim style
                    if let Some(reasoning) = &chunk.reasoning_delta {
                        if !in_reasoning {
                            print!("\x1b[90m[Thinking] ");
                            in_reasoning = true;
                        }
                        reasoning_buf.push_str(reasoning);
                        print!("{}", reasoning);
                    }

                    // Print main content
                    if let Some(delta) = &chunk.text_delta {
                        if in_reasoning {
                            println!("\x1b[0m");
                            in_reasoning = false;
                        }
                        full_text.push_str(delta);
                        print!("{}", delta);
                    }
                }
                if in_reasoning {
                    println!("\x1b[0m");
                }
                println!("\n");

                // Store assistant response in session
                if !full_text.is_empty() {
                    session.add_message(ChatMessage::assistant(&full_text)).await?;
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

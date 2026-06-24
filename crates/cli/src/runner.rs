//! ReplRunner — 开箱即用的 CLI REPL 运行器。
//!
//! ## 最小示例
//!
//! ```ignore
//! use rust_agent_cli::ReplRunner;
//!
//! let agent: Arc<dyn IAgent> = /* ... */;
//! ReplRunner::new(agent).run().await?;
//! ```
//!
//! ## 完整示例（声明式构建 + 模型切换 + 重启）
//!
//! ```ignore
//! use rust_agent_decl::DeclAgentBuilder;
//! use rust_agent_cli::ReplRunner;
//!
//! let agent = DeclAgentBuilder::new()
//!     .from_yaml_file("cli-agent.yaml")
//!     .with_model("agnes-2.0-flash")
//!     .with_api_key(&api_key)
//!     .with_tool("echo", |_| Ok(Arc::new(Echo)))
//!     .with_context(skill_memory.clone())
//!     .build()
//!     .await?;
//!
//! ReplRunner::new(agent)
//!     .prompt("🦀 > ")
//!     .on_switch_model(move |model| { /* rebuild with new model */ })
//!     .on_restart(move || { /* rebuild fresh agent */ })
//!     .run()
//!     .await
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use rust_agent_core::{
    AgentRunOptions, AgentSession, ChatMessage, Content, IAgent,
    ISession, ReasoningEffort, Usage,
};

/// Agent 重建回调（用于 /model 和 /restart 命令）。
pub type SwitchModelFn = Box<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = anyhow::Result<Arc<dyn IAgent>>> + Send>>
        + Send
        + Sync,
>;

pub type RestartFn = Box<
    dyn Fn() -> Pin<Box<dyn Future<Output = anyhow::Result<Arc<dyn IAgent>>> + Send>>
        + Send
        + Sync,
>;

/// ReplRunner — 开箱即用的 CLI REPL 运行器。
///
/// 封装了 rustyline REPL 循环、命令处理、流式输出渲染和 Token 用量统计。
/// 通过 `.on_switch_model()` 和 `.on_restart()` 回调支持运行时 Agent 重建。
#[allow(dead_code)]
pub struct ReplRunner {
    agent: Arc<dyn IAgent>,
    session: Option<Arc<dyn ISession>>,
    prompt: String,
    banner: String,
    thinking_enabled: bool,
    switch_model: Option<SwitchModelFn>,
    restart: Option<RestartFn>,
}

#[allow(dead_code)]
impl ReplRunner {
    /// 创建运行器。`agent` 为要对话的 Agent 实例。
    pub fn new(agent: Arc<dyn IAgent>) -> Self {
        Self {
            agent,
            session: None,
            prompt: "> ".into(),
            banner: String::new(),
            thinking_enabled: true,
            switch_model: None,
            restart: None,
        }
    }

    /// 设置会话。未设置时自动创建 `AgentSession::new()`。
    pub fn session(mut self, session: Arc<dyn ISession>) -> Self {
        self.session = Some(session);
        self
    }

    /// 设置输入提示符，默认 `"> "`。
    pub fn prompt(mut self, p: impl Into<String>) -> Self {
        self.prompt = p.into();
        self
    }

    /// 设置启动横幅，在 REPL 开始时打印。
    pub fn banner(mut self, b: impl Into<String>) -> Self {
        self.banner = b.into();
        self
    }

    /// 设置初始思考模式，默认 `true`。
    pub fn thinking(mut self, enable: bool) -> Self {
        self.thinking_enabled = enable;
        self
    }

    /// 注册 `/model <name>` 回调。回调接收模型名，返回重建后的 Agent。
    /// 未注册时 `/model` 命令不可用。
    pub fn on_switch_model(mut self, f: SwitchModelFn) -> Self {
        self.switch_model = Some(f);
        self
    }

    /// 注册 `/restart` 回调。回调返回重建后的 Agent。
    /// 未注册时 `/restart` 命令不可用。
    pub fn on_restart(mut self, f: RestartFn) -> Self {
        self.restart = Some(f);
        self
    }

    /// 启动 REPL 循环。此方法为阻塞式，直到用户输入 `/quit` 或 `Ctrl+D`。
    pub async fn run(mut self) -> anyhow::Result<()> {
        let session = self
            .session
            .take()
            .unwrap_or_else(|| Arc::new(AgentSession::new()));

        if !self.banner.is_empty() {
            println!("{}", self.banner);
        }
        println!("Type /help for commands, /quit to exit.\n");

        let mut rl = DefaultEditor::new()?;

        loop {
            let line = rl.readline(&self.prompt);
            match line {
                Ok(input) => {
                    let trimmed = input.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let _ = rl.add_history_entry(trimmed);

                    let (handled, rebuild) = self.handle_command(trimmed, &session).await?;
                    if handled {
                        // /restart 或 /model 触发了重建
                        if let Some(new_agent) = rebuild {
                            self.agent = new_agent;
                        }
                        continue;
                    }

                    // Chat
                    self.chat(trimmed, &session).await;
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

    /// 处理 REPL 命令。返回 (已处理, 新Agent)。
    async fn handle_command(
        &mut self,
        input: &str,
        session: &Arc<dyn ISession>,
    ) -> anyhow::Result<(bool, Option<Arc<dyn IAgent>>)> {
        match input {
            "/quit" | "/exit" | "quit" | "exit" => {
                println!("Bye!");
                std::process::exit(0);
            }

            "/help" => {
                self.print_help();
                Ok((true, None))
            }

            "/clear" => {
                session.clear().await?;
                println!("[History cleared]\n");
                Ok((true, None))
            }

            "/restart" => {
                if let Some(ref restart_fn) = self.restart {
                    session.clear().await?;
                    let new_agent = restart_fn().await?;
                    println!("[Session restarted — history cleared, agent rebuilt]\n");
                    Ok((true, Some(new_agent)))
                } else {
                    println!("/restart requires .on_restart() callback\n");
                    Ok((true, None))
                }
            }

            arg if arg.starts_with("/think") => {
                match arg.trim() {
                    "/think on" => {
                        self.thinking_enabled = true;
                        println!("[Thinking mode enabled]\n");
                    }
                    "/think off" => {
                        self.thinking_enabled = false;
                        println!("[Thinking mode disabled]\n");
                    }
                    _ => println!("Usage: /think on|off\n"),
                }
                Ok((true, None))
            }

            arg if arg.starts_with("/model") => {
                let model = arg.strip_prefix("/model").unwrap().trim();
                if model.is_empty() {
                    println!("Usage: /model <name>\n");
                } else if let Some(ref switch_fn) = self.switch_model {
                    match switch_fn(model.to_string()).await {
                        Ok(new_agent) => {
                            println!("[Model switched to {}]\n", model);
                            return Ok((true, Some(new_agent)));
                        }
                        Err(e) => {
                            println!("[Error switching model: {}]\n", e);
                        }
                    }
                } else {
                    println!("/model requires .on_switch_model() callback\n");
                }
                Ok((true, None))
            }

            _ => Ok((false, None)),
        }
    }

    /// 执行一次 Chat 请求并渲染流式输出。
    async fn chat(&self, input: &str, session: &Arc<dyn ISession>) {
        let messages = vec![ChatMessage::user(input)];

        let mut run_opts = AgentRunOptions::new();
        if self.thinking_enabled {
            run_opts = run_opts
                .with_thinking(true)
                .with_reasoning_effort(ReasoningEffort::High);
        }

        let result = self
            .agent
            .run(messages, Some(session.clone()), Some(run_opts))
            .await;

        match result {
            Ok(mut stream) => {
                let mut in_reasoning = false;
                let mut active_tools: HashMap<String, String> = HashMap::new();
                let mut accumulated_usage: Option<Usage> = None;

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
                                    Content::ToolCallStart(c) => {
                                        if in_reasoning {
                                            println!("\x1b[0m");
                                            in_reasoning = false;
                                        }
                                        let label = format!("\x1b[36m[调用] {}\x1b[0m", c.name);
                                        eprintln!("\n{} 参数:", label);
                                        active_tools.insert(c.call_id.clone(), label);
                                    }
                                    Content::ToolCallArgs(_c) => {}
                                    Content::ToolCallArgsParsed(c) => {
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
                                        eprintln!("  \x1b[32m{}\x1b[0m = {}", c.name, val_str);
                                    }
                                    Content::ToolCallArgsProgress(c) => {
                                        let preview = c.value.as_str().unwrap_or("");
                                        let preview = if preview.chars().count() > 60 {
                                            let tail: String = preview
                                                .chars()
                                                .rev()
                                                .take(60)
                                                .collect::<Vec<_>>()
                                                .into_iter()
                                                .rev()
                                                .collect();
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
                                        let _ = active_tools.remove(&_c.call_id);
                                    }
                                    Content::ToolCalling(c) => {
                                        if in_reasoning {
                                            println!("\x1b[0m");
                                            in_reasoning = false;
                                        }
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
                                        eprintln!(
                                            "\n\x1b[31m[错误]\x1b[0m {}: {}",
                                            c.error_code, c.message
                                        );
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
                                    rust_agent_core::FinishReason::Stop => {}
                                    rust_agent_core::FinishReason::ToolCalls => {}
                                    _ => {
                                        eprintln!("\n\x1b[90m[结束] {:?}\x1b[0m", reason);
                                    }
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

                // Token usage summary（独立换行，避免与流式正文粘连）
                if let Some(ref usage) = accumulated_usage {
                    print_usage_summary(usage);
                }
                println!();
            }
            Err(e) => {
                eprintln!("[错误] {}", e);
            }
        }
    }

    fn print_help(&self) {
        println!("Commands:");
        println!("  /help        Show this help");
        println!("  /clear       Clear conversation history");
        if self.restart.is_some() {
            println!("  /restart     Clear history + rebuild agent");
        }
        println!("  /think on    Enable thinking mode");
        println!("  /think off   Disable thinking mode");
        if self.switch_model.is_some() {
            println!("  /model NAME  Switch model (e.g. agnes-2.0-flash)");
        }
        println!("  /quit|exit   Exit (also: quit, exit without slash)");
    }
}

/// 在流式正文之后单独打印用量块（保证换行，并展示 KV cache 命中/未命中）。
fn print_usage_summary(usage: &Usage) {
    let reasoning = usage.reasoning_tokens.unwrap_or(0);

    // 流式输出用 print! 无尾换行，先补一行再打印用量
    println!();
    println!(
        "\x1b[90m[用量] prompt={}  completion={}  total={}\x1b[0m",
        usage.prompt_tokens, usage.completion_tokens, usage.total_tokens,
    );
    if usage.cache_stats_available() {
        let hit = usage.cache_hit_tokens();
        let miss = usage.cache_miss_tokens();
        let cache_ratio = usage.cache_hit_ratio() * 100.0;
        println!(
            "\x1b[90m       cache hit={}  miss={}  ({:.1}%)\x1b[0m",
            hit, miss, cache_ratio,
        );
    } else {
        println!("\x1b[90m       cache: (供应商未上报)\x1b[0m");
    }
    if reasoning > 0 {
        println!("\x1b[90m       reasoning={}\x1b[0m", reasoning);
    }
    if let Some(ref raw) = usage.raw {
        println!("\x1b[90m       usage (raw): {}\x1b[0m", raw);
    }
    let _ = std::io::stdout().flush();
}

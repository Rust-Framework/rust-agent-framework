//! rust-agent-cli — Interactive Chat (DeepSeek)
//!
//! 使用声明式 YAML + DeclAgentBuilder 构建 Agent，
//! 通过 ReplRunner 提供 REPL 交互界面。

use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::ToolResult;
use rust_agent_decl::DeclAgentBuilder;
use rust_agent_framework::memory::SkillMemoryContextProvider;
use rust_agent_framework::tool;

mod runner;
use runner::ReplRunner;

// ── API Key ────────────────────────────────────────────────────
const DEEPSEEK_API_KEY: &str = "sk-b8136a230aea467e8cdfe4649cab2d3e";

fn cli_agent_yaml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cli-agent.yaml")
}

fn api_key() -> String {
    std::env::var("DEEPSEEK_API_KEY")
        .unwrap_or_else(|_| DEEPSEEK_API_KEY.to_string())
}

// ── Tool definitions ───────────────────────────────────────────
#[tool(description = "Echoes back the input text")]
async fn echo(#[param(desc = "The text to echo")] text: String) -> ToolResult {
    ToolResult::success(serde_json::json!({"echo": text}))
}

#[tool(description = "Adds two numbers together")]
async fn add(#[param(desc = "First number")] a: i64, #[param(desc = "Second number")] b: i64) -> ToolResult {
    ToolResult::success(serde_json::json!({"result": a + b}))
}

// ── Agent 构建 ─────────────────────────────────────────────────

async fn build_agent(
    model_id: &str,
    skill_memory: &Arc<SkillMemoryContextProvider>,
) -> anyhow::Result<Arc<dyn rust_agent_core::IAgent>> {
    DeclAgentBuilder::new()
        .from_yaml_file(cli_agent_yaml_path())
        .with_model(model_id)
        .with_api_key(&api_key())
        .with_tool("echo", |_| Ok(Arc::new(Echo)))
        .with_tool("add", |_| Ok(Arc::new(Add)))
        .with_context(Arc::clone(skill_memory) as Arc<dyn rust_agent_core::IContextProvider>)
        .build()
        .await
        .map_err(Into::into)
}

// ── main ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("warn".parse()?),
        )
        .init();

    let memory_dir = PathBuf::from("logs/memory");
    std::fs::create_dir_all(&memory_dir).ok();

    let skill_memory = Arc::new(
        SkillMemoryContextProvider::new(&memory_dir).with_consolidation_interval(1),
    );

    // 构建 Agent
    let agent = build_agent("deepseek-v4-flash", &skill_memory).await?;

    // 启动 REPL
    let sm = Arc::clone(&skill_memory);
    let key = api_key();
    let sm2 = Arc::clone(&skill_memory);
    let key2 = api_key();

    ReplRunner::new(agent)
        .banner("rust-agent-cli — Declarative Chat (DeepSeek)")
        .on_switch_model(Box::new(move |model| {
            let sm = Arc::clone(&sm);
            let key = key.clone();
            Box::pin(async move {
                let a = DeclAgentBuilder::new()
                    .from_yaml_file(cli_agent_yaml_path())
                    .with_model(&model)
                    .with_api_key(&key)
                    .with_tool("echo", |_| Ok(Arc::new(Echo)))
                    .with_tool("add", |_| Ok(Arc::new(Add)))
                    .with_context(sm as Arc<dyn rust_agent_core::IContextProvider>)
                    .build()
                    .await?;
                Ok(a)
            })
        }))
        .on_restart(Box::new(move || {
            let sm = Arc::clone(&sm2);
            let key = key2.clone();
            Box::pin(async move {
                let a = DeclAgentBuilder::new()
                    .from_yaml_file(cli_agent_yaml_path())
                    .with_model("deepseek-v4-flash")
                    .with_api_key(&key)
                    .with_tool("echo", |_| Ok(Arc::new(Echo)))
                    .with_tool("add", |_| Ok(Arc::new(Add)))
                    .with_context(sm as Arc<dyn rust_agent_core::IContextProvider>)
                    .build()
                    .await?;
                Ok(a)
            })
        }))
        .run()
        .await
}

//! rust-agent-cli — Interactive Chat (DeepSeek)
//!
//! 使用声明式 YAML + DeclAgentBuilder 构建 Agent，
//! 通过 ReplRunner 提供 REPL 交互界面。
//! 所有配置（模型、API Key、上下文提供器、工具）均从 YAML 读取。

use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_core::ToolResult;
use rust_agent_decl::DeclAgentBuilder;
use rust_agent_framework::tool;

mod runner;
use runner::ReplRunner;

fn cli_agent_yaml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cli-agent.yaml")
}

// ── Tool definitions ───────────────────────────────────────────
#[tool(description = "将输入文本原样返回")]
async fn echo(#[param(desc = "要回显的文本")] text: String) -> ToolResult {
    ToolResult::success(serde_json::json!({"echo": text}))
}

#[tool(description = "将两个数字相加")]
async fn add(#[param(desc = "第一个数字")] a: i64, #[param(desc = "第二个数字")] b: i64) -> ToolResult {
    ToolResult::success(serde_json::json!({"result": a + b}))
}

// ── Agent 构建 ─────────────────────────────────────────────────

async fn build_agent() -> anyhow::Result<Arc<dyn rust_agent_core::IAgent>> {
    DeclAgentBuilder::new()
        .from_yaml_file(cli_agent_yaml_path())
        .with_tool("echo", |_| Ok(Arc::new(Echo)))
        .with_tool("add", |_| Ok(Arc::new(Add)))
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

    // 构建 Agent — 模型、API Key、contextProviders 全部从 YAML 读取
    let agent = build_agent().await?;

    // 启动 REPL
    ReplRunner::new(agent)
        .banner("rust-agent-cli — 声明式聊天助手 (DeepSeek)")
        .on_switch_model(Box::new(move |model| {
            Box::pin(async move {
                let a = DeclAgentBuilder::new()
                    .from_yaml_file(cli_agent_yaml_path())
                    .with_model(&model)
                    .with_tool("echo", |_| Ok(Arc::new(Echo)))
                    .with_tool("add", |_| Ok(Arc::new(Add)))
                    .build()
                    .await?;
                Ok(a)
            })
        }))
        .on_restart(Box::new(move || {
            Box::pin(async move {
                let a = DeclAgentBuilder::new()
                    .from_yaml_file(cli_agent_yaml_path())
                    .with_tool("echo", |_| Ok(Arc::new(Echo)))
                    .with_tool("add", |_| Ok(Arc::new(Add)))
                    .build()
                    .await?;
                Ok(a)
            })
        }))
        .run()
        .await
}

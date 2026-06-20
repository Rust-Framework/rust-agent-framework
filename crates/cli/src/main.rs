//! rust-agent-cli — Interactive Chat (DeepSeek)
//!
//! 使用声明式 YAML + DeclAgentBuilder 构建 Agent，
//! 通过 ReplRunner 提供 REPL 交互界面。
//! 所有配置（模型、API Key、上下文提供器、工具）均从 YAML 读取。

use std::path::PathBuf;
use std::sync::Arc;

use rust_agent_decl::DeclAgentBuilder;

mod runner;
use runner::ReplRunner;

fn cli_agent_yaml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cli-agent.yaml")
}

// ── Agent 构建 ─────────────────────────────────────────────────

async fn build_agent() -> anyhow::Result<Arc<dyn rust_agent_core::IAgent>> {
    DeclAgentBuilder::new()
        .from_yaml_file(cli_agent_yaml_path())
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
                    .build()
                    .await?;
                Ok(a)
            })
        }))
        .on_restart(Box::new(move || {
            Box::pin(async move {
                let a = DeclAgentBuilder::new()
                    .from_yaml_file(cli_agent_yaml_path())
                    .build()
                    .await?;
                Ok(a)
            })
        }))
        .run()
        .await
}

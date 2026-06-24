//! rust-agent-cli — Interactive Chat
//!
//! 使用声明式 YAML + DeclAgentBuilder 构建 Agent，
//! 通过 ReplRunner 提供 REPL 交互界面。
//!
//! ```text
//! cargo run -p rust-agent-cli                              # cli-agent.yaml
//! cargo run -p rust-agent-cli -- --config local-agent.yaml # 本地 GGUF
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rust_agent_decl::DeclAgentBuilder;

mod runner;
use runner::ReplRunner;

fn default_agent_yaml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cli-agent.yaml")
}

/// 解析 Agent 配置文件路径：`--config <path>`，否则默认 `cli-agent.yaml`。
fn agent_config_path() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--config" || a == "-c") {
        if let Some(path) = args.get(pos + 1) {
            let p = PathBuf::from(path);
            if p.is_absolute() || p.exists() {
                return p;
            }
            let in_crate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
            if in_crate.exists() {
                return in_crate;
            }
            return p;
        }
    }
    default_agent_yaml_path()
}

fn config_banner(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("agent.yaml");
    format!("rust-agent-cli — {name}")
}

async fn build_agent_from(path: PathBuf) -> anyhow::Result<Arc<dyn rust_agent_core::IAgent>> {
    DeclAgentBuilder::new()
        .from_yaml_file(path)
        .build()
        .await
        .map_err(Into::into)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("warn".parse()?),
        )
        .init();

    let config_path = agent_config_path();
    let banner = config_banner(&config_path);

    let agent = build_agent_from(config_path.clone()).await?;

    let config_for_rebuild = config_path.clone();
    ReplRunner::new(agent)
        .banner(&banner)
        .thinking(false)
        .on_switch_model(Box::new(move |_model| {
            let path = config_for_rebuild.clone();
            Box::pin(async move {
                build_agent_from(path).await
            })
        }))
        .on_restart({
            let path = config_path.clone();
            Box::new(move || {
                let path = path.clone();
                Box::pin(async move { build_agent_from(path).await })
            })
        })
        .run()
        .await
}

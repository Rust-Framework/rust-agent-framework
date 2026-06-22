//! Declarative agent loader — load agents from JSON/YAML/TOML files.
//!
//! Scans a directory for agent declaration files, parses `AgentDecl`,
//! resolves them into `IAgent` instances using the `rust-agent-decl` resolver.
//!
//! 自动注入工作区管控（WorkspaceContextProvider）和记忆系统
//! （SkillMemoryContextProvider），与内置 Agent 保持一致的能力水平。

use std::path::Path;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn, debug};

use rust_agent_core::{IAgent, IContextProvider, ScopePolicy, WorkspaceScope};
use rust_agent_decl::DeclAgentBuilder;
use rust_agent_framework::{WorkspaceContextProvider, memory::SkillMemoryContextProvider};

use crate::config::HostConfig;

/// Loader for declarative agent files.
pub struct DeclLoader<'a> {
    /// Directory containing agent declaration files.
    agents_dir: &'a str,
    /// Host configuration —用于注入工作区和记忆 provider。
    config: &'a HostConfig,
}

impl<'a> DeclLoader<'a> {
    pub fn new(agents_dir: &'a str, config: &'a HostConfig) -> Self {
        Self { agents_dir, config }
    }

    /// Load all agent declarations from the directory.
    ///
    /// 每个声明式 Agent 自动注入：
    /// 1. **WorkspaceContextProvider** — 工作区 scope 管控和审批策略
    /// 2. **SkillMemoryContextProvider** — 跨会话持久记忆（如果配置了 memory_dir）
    pub async fn load_all(&self) -> Result<Vec<Arc<dyn IAgent>>> {
        let dir = Path::new(self.agents_dir);
        if !dir.exists() || !dir.is_dir() {
            warn!(agents_dir = %self.agents_dir, "Agents directory not found or not a directory");
            return Ok(Vec::new());
        }

        // 预构建可复用的上下文提供器
        let workspace_provider: Option<Arc<dyn IContextProvider>> = {
            let policy = ScopePolicy::from_config_str(&self.config.scope_policy);
            let scope = Arc::new(
                WorkspaceScope::new(&self.config.workspace_root, "workspace")
                    .with_policy(policy),
            );
            Some(Arc::new(WorkspaceContextProvider::new(scope)))
        };

        let memory_provider: Option<Arc<dyn IContextProvider>> = self
            .config
            .memory_dir
            .as_ref()
            .map(|dir| {
                debug!(memory_dir = %dir, "Creating memory provider for declarative agents");
                Arc::new(SkillMemoryContextProvider::new(dir)) as Arc<dyn IContextProvider>
            });

        let mut agents = Vec::new();

        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Skip non-files
            if !path.is_file() {
                continue;
            }

            let mut builder = DeclAgentBuilder::new().from_file(&path);

            // 注入工作区上下文提供器
            if let Some(ref ws) = workspace_provider {
                builder = builder.with_context(ws.clone());
            }

            // 注入记忆上下文提供器
            if let Some(ref mem) = memory_provider {
                builder = builder.with_context(mem.clone());
            }

            match builder.build().await {
                Ok(agent) => {
                    let file_name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("(unknown)");
                    info!(agent_id = file_name, path = %path.display(), "Loaded declarative agent");
                    agents.push(agent);
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to load declarative agent");
                }
            }
        }

        Ok(agents)
    }
}

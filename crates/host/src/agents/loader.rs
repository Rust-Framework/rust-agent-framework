//! Declarative agent loader — load agents from JSON/YAML/TOML files.
//!
//! Scans a directory for agent declaration files, parses `AgentDecl`,
//! resolves them into `IAgent` instances using the `rust-agent-decl` resolver.

use std::path::Path;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};

use rust_agent_core::IAgent;
use rust_agent_decl::DeclAgentBuilder;

use crate::config::HostConfig;

/// Loader for declarative agent files.
pub struct DeclLoader<'a> {
    /// Directory containing agent declaration files.
    agents_dir: &'a str,
    /// Host configuration (for provider fallback).
    config: &'a HostConfig,
}

impl<'a> DeclLoader<'a> {
    pub fn new(agents_dir: &'a str, config: &'a HostConfig) -> Self {
        Self { agents_dir, config }
    }

    /// Load all agent declarations from the directory.
    pub async fn load_all(&self) -> Result<Vec<Arc<dyn IAgent>>> {
        let dir = Path::new(self.agents_dir);
        if !dir.exists() || !dir.is_dir() {
            warn!(agents_dir = %self.agents_dir, "Agents directory not found or not a directory");
            return Ok(Vec::new());
        }

        let mut agents = Vec::new();

        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Skip non-files
            if !path.is_file() {
                continue;
            }

            // 使用 DeclAgentBuilder::from_file() 统一加载（自动检测 YAML/JSON/TOML）
            match DeclAgentBuilder::new()
                .from_file(&path)
                .build()
                .await
            {
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

    /// }

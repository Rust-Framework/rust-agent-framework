//! Declarative agent loader — load agents from JSON/YAML/TOML files.
//!
//! Scans a directory for agent declaration files, parses `AgentDecl`,
//! resolves them into `IAgent` instances using the `rust-agent-decl` resolver.

use std::path::Path;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn};

use rust_agent_core::IAgent;
use rust_agent_decl::{AgentDecl, AgentResolver, DefaultAgentResolver};

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

        let resolver = DefaultAgentResolver::new();
        let mut agents = Vec::new();

        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Skip non-files
            if !path.is_file() {
                continue;
            }

            // Determine file format by extension
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let decl = match extension {
                "json" => {
                    let content = std::fs::read_to_string(&path)?;
                    AgentDecl::from_json_str(&content)?
                }
                #[cfg(feature = "yaml")]
                "yaml" | "yml" => {
                    let content = std::fs::read_to_string(&path)?;
                    AgentDecl::from_yaml_str(&content)?
                }
                #[cfg(feature = "toml")]
                "toml" => {
                    let content = std::fs::read_to_string(&path)?;
                    AgentDecl::from_toml_str(&content)?
                }
                _ => {
                    warn!(path = %path.display(), extension, "Unsupported file extension, skipping");
                    continue;
                }
            };

            // Resolve model config: if the decl doesn't specify a model, fall back to host config
            let decl = self.patch_model_config(decl);

            match resolver.resolve(&decl).await {
                Ok(agent) => {
                    info!(agent_id = %decl.id, path = %path.display(), "Loaded declarative agent");
                    agents.push(agent);
                }
                Err(e) => {
                    warn!(agent_id = %decl.id, path = %path.display(), error = %e, "Failed to resolve declarative agent");
                }
            }
        }

        Ok(agents)
    }

    /// Patch the model config: if the declaration doesn't specify a model,
    /// inject the host's default provider config.
    fn patch_model_config(&self, mut decl: AgentDecl) -> AgentDecl {
        // If the declaration has no api_key explicitly set, fall back to host config
        if decl.model.api_key.is_none() {
            if let Some(ref key) = self.config.provider.api_key {
                decl.model.api_key = Some(key.clone());
            }
        }
        // If no base_url, fall back
        if decl.model.base_url.is_none() {
            decl.model.base_url = self.config.provider.base_url.clone();
        }
        // If no temperature, fall back
        if decl.model.temperature.is_none() {
            decl.model.temperature = self.config.provider.temperature;
        }
        // If no max_tokens, fall back
        if decl.model.max_tokens.is_none() {
            decl.model.max_tokens = self.config.provider.max_tokens;
        }
        decl
    }
}

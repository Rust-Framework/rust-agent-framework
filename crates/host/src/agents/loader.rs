//! Declarative agent loader — load agents from JSON/YAML/TOML files.

use std::path::Path;
use std::sync::Arc;
use anyhow::Result;
use tracing::{info, warn, debug};

use rust_agent_core::{IAgent, IContextProvider, ScopePolicy, WorkspaceScope};
use rust_agent_decl::DeclAgentBuilder;
use rust_agent_framework::{bundle::BundleProvider, WorkspaceContextProvider};

use crate::config::HostConfig;

/// Loader for declarative agent files.
pub struct DeclLoader<'a> {
    agents_dir: &'a str,
    config: &'a HostConfig,
}

impl<'a> DeclLoader<'a> {
    pub fn new(agents_dir: &'a str, config: &'a HostConfig) -> Self {
        Self { agents_dir, config }
    }

    pub async fn load_all(&self) -> Result<Vec<Arc<dyn IAgent>>> {
        let dir = Path::new(self.agents_dir);
        if !dir.exists() || !dir.is_dir() {
            warn!(agents_dir = %self.agents_dir, "Agents directory not found or not a directory");
            return Ok(Vec::new());
        }

        let workspace_provider: Option<Arc<dyn IContextProvider>> = {
            let policy = ScopePolicy::from_config_str(&self.config.scope_policy);
            let scope = Arc::new(
                WorkspaceScope::new(&self.config.workspace_root, "workspace")
                    .with_policy(policy),
            );
            Some(Arc::new(WorkspaceContextProvider::new(scope)))
        };

        let bundle_provider: Option<Arc<dyn IContextProvider>> = self
            .config
            .bundle_dir
            .as_ref()
            .map(|dir| {
                debug!(bundle_dir = %dir, "Creating knowledge bundle provider for declarative agents");
                Arc::new(BundleProvider::new(dir)) as Arc<dyn IContextProvider>
            });

        let mut agents = Vec::new();

        let entries = std::fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            let mut builder = DeclAgentBuilder::new().from_file(&path);

            if let Some(ref ws) = workspace_provider {
                builder = builder.with_context(ws.clone());
            }

            if let Some(ref bundle) = bundle_provider {
                builder = builder.with_context(bundle.clone());
            }

            match builder.build().await {
                Ok(agent) => {
                    let file_name = path
                        .file_stem()
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

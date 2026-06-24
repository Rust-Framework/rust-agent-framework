use std::sync::Arc;

use rust_agent_core::{IChatClient, IContextProvider, ITool, ScopePolicy, WorkspaceScope};
use rust_agent_framework::WorkspaceContextProvider;

use crate::context_provider_config::ContextProviderDecl;

/// Build a context provider from a declarative `(kind, name, config)` tuple.
///
/// Framework builtins (`super-brain`, `skills`, `workspace`) are always available.
/// Optional integrations require the matching Cargo feature:
/// `mcp`, `rag`, `wiki`.
pub fn build_provider_from_decl(
    decl: &ContextProviderDecl,
    curator_client: Option<Arc<dyn IChatClient>>,
) -> crate::Result<Option<Arc<dyn IContextProvider>>> {
    match decl {
        ContextProviderDecl::Memory { name, config } if name == "super-brain" => {
            let dir = config
                .get("directory")
                .and_then(|v| v.as_str())
                .unwrap_or("logs/super-brain");
            let enabled = config
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let interval = config
                .get("consolidationInterval")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;

            let super_brain_dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&super_brain_dir).ok();

            let memory_client = match crate::resolver::memory_model_resolver::resolve_super_brain_memory_client(config) {
                Ok(dedicated) => dedicated.or(curator_client),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "super-brain memoryModel failed to load; falling back to main agent client for memory consolidation"
                    );
                    curator_client
                }
            };

            let mut provider = rust_agent_framework::super_brain::SuperBrainContextProvider::new(&super_brain_dir)
                .with_enabled(enabled)
                .with_consolidation_interval(interval);
            if let Some(client) = memory_client {
                provider = provider.with_curator_client(client);
            }
            Ok(Some(Arc::new(provider)))
        }

        ContextProviderDecl::Skills { name: skill_name, config } => {
            let dir = config
                .get("directory")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let dir_path = if dir.is_empty() {
                std::path::PathBuf::from("skills").join(skill_name)
            } else {
                std::path::PathBuf::from(dir)
            };

            match rust_agent_framework::AgentSkillsProvider::scan(&dir_path) {
                Ok(provider) => {
                    if provider.skills.is_empty() {
                        tracing::warn!(
                            "No SKILL.md found in skills directory '{}' for skill '{}'",
                            dir_path.display(),
                            skill_name
                        );
                    }
                    Ok(Some(Arc::new(provider)))
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to scan skills directory '{}' for skill '{}': {}",
                        dir_path.display(),
                        skill_name,
                        e
                    );
                    Ok(None)
                }
            }
        }

        ContextProviderDecl::Mcp { name: server_name, config } => {
            let server_url = config
                .get("serverUrl")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let server_command = config.get("command").and_then(|v| v.as_str());
            let server_args = config
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect::<Vec<_>>()
                });

            let _ = server_command;
            let _ = server_args;
            #[cfg(feature = "mcp")]
            {
                tracing::error!(
                    "MCP declarative provider requires async connection and cannot be constructed \
                     in build_provider_from_decl. Use DeclAgentBuilder::with_context() to inject a \
                     pre-connected McpContextProvider, or enable the `mcp` feature and \
                     AgentBuilderMcpExt::with_mcp_server(). \
                     Server: '{}', URL: '{}'",
                    server_name,
                    if server_url.is_empty() {
                        "(not specified)"
                    } else {
                        server_url
                    }
                );
            }
            #[cfg(not(feature = "mcp"))]
            {
                tracing::error!(
                    "MCP context provider requires decl `mcp` feature. \
                     Server: '{}', URL: '{}'",
                    server_name,
                    if server_url.is_empty() {
                        "(not specified)"
                    } else {
                        server_url
                    }
                );
            }
            Ok(None)
        }

        ContextProviderDecl::Workspace { name: ws_name, config } => {
            let root = config
                .get("root")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            let policy_str = config
                .get("policy")
                .and_then(|v| v.as_str())
                .unwrap_or("approve");

            let policy = ScopePolicy::from_config_str(policy_str);
            if policy == ScopePolicy::DenyOutside
                && policy_str != "deny_outside"
                && policy_str != "deny"
                && policy_str != "restrict"
            {
                tracing::error!(
                    "Unknown workspace policy '{}' for '{}', falling back to DenyOutside (fail closed). \
                     Valid values: read/allow/allow_all, approve/ask/approve_outside, deny/restrict/deny_outside",
                    policy_str, ws_name
                );
            }

            let scope = WorkspaceScope::new(root, ws_name.as_str()).with_policy(policy);
            let provider = WorkspaceContextProvider::new(Arc::new(scope));
            Ok(Some(Arc::new(provider)))
        }

        ContextProviderDecl::Knowledge { name: kb_name, config } => {
            let source = config
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            #[cfg(feature = "rag")]
            {
                if source.is_empty() {
                    tracing::warn!(
                        "Knowledge provider '{}' missing config.source — skipped",
                        kb_name
                    );
                    return Ok(None);
                }
                return Ok(Some(Arc::new(rust_agent_rag::RagContextProvider::new(
                    kb_name.clone(),
                    source,
                ))));
            }
            #[cfg(not(feature = "rag"))]
            {
                tracing::error!(
                    "Knowledge (RAG) requires decl `rag` feature. \
                     Base: '{}', source: '{}'",
                    kb_name,
                    if source.is_empty() {
                        "(not specified)"
                    } else {
                        source
                    }
                );
                Ok(None)
            }
        }

        ContextProviderDecl::Wiki { name: wiki_name, config } => {
            let source = config
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            #[cfg(feature = "wiki")]
            {
                if source.is_empty() {
                    tracing::warn!(
                        "Wiki provider '{}' missing config.source — skipped",
                        wiki_name
                    );
                    return Ok(None);
                }
                return Ok(Some(Arc::new(rust_agent_wiki::WikiContextProvider::new(
                    wiki_name.clone(),
                    source,
                ))));
            }
            #[cfg(not(feature = "wiki"))]
            {
                tracing::error!(
                    "Wiki provider requires decl `wiki` feature. Wiki: '{}', source: '{}'",
                    wiki_name,
                    if source.is_empty() {
                        "(not specified)"
                    } else {
                        source
                    }
                );
                Ok(None)
            }
        }

        ContextProviderDecl::Memory { .. } => {
            tracing::debug!(
                "Unknown memory provider name (expected 'super-brain')"
            );
            Ok(None)
        }
    }
}

/// Build workspace provider and attach scope-aware tools.
pub fn build_workspace_provider(
    decl: &ContextProviderDecl,
    scope_tools: &[Arc<dyn ITool>],
) -> crate::Result<Option<Arc<dyn IContextProvider>>> {
    let (ws_name, config) = match decl {
        ContextProviderDecl::Workspace { name, config } => (name, config),
        _ => return build_provider_from_decl(decl, None),
    };

    let root = config
        .get("root")
        .and_then(|v| v.as_str())
        .unwrap_or(".");
    let policy_str = config
        .get("policy")
        .and_then(|v| v.as_str())
        .unwrap_or("approve");

    let policy = ScopePolicy::from_config_str(policy_str);
    if policy == ScopePolicy::DenyOutside
        && policy_str != "deny_outside"
        && policy_str != "deny"
        && policy_str != "restrict"
    {
        tracing::error!(
            "Unknown workspace policy '{}' for '{}', falling back to DenyOutside (fail closed). \
             Valid values: read/allow/allow_all, approve/ask/approve_outside, deny/restrict/deny_outside",
            policy_str, ws_name
        );
    }

    let scope = WorkspaceScope::new(root, ws_name.as_str()).with_policy(policy);
    let mut provider = WorkspaceContextProvider::new(Arc::new(scope));
    for tool in scope_tools {
        provider.add_tool_arc(Arc::clone(tool));
    }

    if !scope_tools.is_empty() {
        tracing::debug!(
            "Workspace '{}' managing {} IScopeTool(s): {}",
            ws_name,
            scope_tools.len(),
            scope_tools
                .iter()
                .map(|t| t.name())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(Some(Arc::new(provider)))
}

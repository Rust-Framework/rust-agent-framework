//! Multi-agent registration center with sub-agent discovery.
//!
//! Stores `Arc<dyn IAgent>` instances and provides:
//! - Registration and lookup by `AgentId`
//! - Recursive sub-agent discovery via `get_subagent()`
//! - Agent list metadata for ACP `initialize` response

use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{AgentId, IAgent};
use serde::Serialize;

/// Information about a registered agent, exposed via `_raf/agent_list` and `initialize._meta`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub agent_type: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tool_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capability_tags: Vec<String>,
    #[serde(default)]
    pub has_subagents: bool,
    /// This agent is the default (used when no agent_id is specified in session/new).
    #[serde(default)]
    pub is_default: bool,
}

/// Information about a sub-agent, exposed via `_raf/subagent_list`.
#[derive(Debug, Clone, Serialize)]
pub struct SubAgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capability_tags: Vec<String>,
    /// Depth in the agent tree (0 = direct child of queried agent).
    pub depth: usize,
    #[serde(default)]
    pub has_subagents: bool,
}

/// Tree node for `_raf/subagent_tree`.
#[derive(Debug, Clone, Serialize)]
pub struct SubAgentNode {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub capability_tags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<SubAgentNode>,
}

/// Multi-agent registry.
pub struct AgentRegistry {
    agents: HashMap<AgentId, Arc<dyn IAgent>>,
    /// Default agent ID (the first registered agent).
    default_id: Option<AgentId>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            default_id: None,
        }
    }

    /// Register an agent.
    pub fn register(&mut self, agent: Arc<dyn IAgent>) {
        let id = agent.id().clone();
        if self.default_id.is_none() {
            self.default_id = Some(id.clone());
        }
        self.agents.insert(id, agent);
    }

    /// Look up an agent by ID.
    pub fn get(&self, id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.agents.get(id)
    }

    /// Get the default agent.
    pub fn get_default(&self) -> Option<&Arc<dyn IAgent>> {
        self.default_id.as_ref().and_then(|id| self.agents.get(id))
    }

    /// Return all registered agent IDs.
    pub fn ids(&self) -> Vec<&AgentId> {
        self.agents.keys().collect()
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Build agent list info for all registered agents.
    pub fn build_agent_list(&self) -> Vec<AgentInfo> {
        let mut list: Vec<AgentInfo> = self
            .agents
            .iter()
            .map(|(id, agent)| {
                let meta = agent.metadata();
                AgentInfo {
                    id: id.to_string(),
                    agent_type: meta.agent_type.clone(),
                    name: meta.key.clone(),
                    description: meta.description.clone(),
                    tool_names: meta.tool_names.clone(),
                    model_id: meta.model_id.clone(),
                    capability_tags: meta.capability_tags.clone(),
                    has_subagents: self.count_subagents(agent) > 0,
                    is_default: self.default_id.as_ref() == Some(id),
                }
            })
            .collect();
        // Sort: default first, then by id
        list.sort_by(|a, b| {
            b.is_default
                .cmp(&a.is_default)
                .then_with(|| a.id.cmp(&b.id))
        });
        list
    }

    /// Build agent list as `_meta` value for ACP `initialize` response.
    pub fn build_agent_list_meta(&self) -> serde_json::Value {
        let agents = self.build_agent_list();
        serde_json::json!({
            "raf": {
                "agents": agents,
                "version": "0.1.0"
            }
        })
    }

    /// Get sub-agent list for a given agent by recursively calling `get_subagent()`.
    pub fn get_subagent_list(&self, agent_id: &AgentId) -> Vec<SubAgentInfo> {
        let agent = match self.agents.get(agent_id) {
            Some(a) => a,
            None => return vec![],
        };
        let mut result = Vec::new();
        self.collect_subagents(agent, &mut result, 0);
        result
    }

    /// Get sub-agent tree for a given agent.
    pub fn get_subagent_tree(&self, agent_id: &AgentId) -> Option<SubAgentNode> {
        let agent = self.agents.get(agent_id)?;
        Some(self.build_subagent_node(agent))
    }

    /// Resolve the target agent for a request. Checks `_meta.raf.agent_id`, falls back to default.
    pub fn resolve_agent(&self, agent_id_override: Option<&str>) -> Option<Arc<dyn IAgent>> {
        if let Some(id_str) = agent_id_override {
            let id = AgentId::new(id_str);
            // First, try direct lookup
            if let Some(agent) = self.agents.get(&id) {
                return Some(agent.clone());
            }
            // Then, try sub-agent lookup across all registered agents
            for parent in self.agents.values() {
                if let Some(sub) = parent.get_subagent(&id) {
                    return Some(sub);
                }
            }
        }
        // Fall back to default
        self.get_default().cloned()
    }

    // ── Private helpers ──

    fn count_subagents(&self, agent: &Arc<dyn IAgent>) -> usize {
        self.agents
            .keys()
            .filter(|id| agent.get_subagent(id).is_some())
            .count()
    }

    fn collect_subagents(
        &self,
        agent: &Arc<dyn IAgent>,
        out: &mut Vec<SubAgentInfo>,
        depth: usize,
    ) {
        for (child_id, child_agent) in &self.agents {
            if agent.get_subagent(child_id).is_some() {
                let meta = child_agent.metadata();
                let has_sub = self.count_subagents(child_agent) > 0;
                out.push(SubAgentInfo {
                    id: child_id.to_string(),
                    name: meta.key.clone(),
                    agent_type: meta.agent_type.clone(),
                    description: meta.description.clone(),
                    capability_tags: meta.capability_tags.clone(),
                    depth,
                    has_subagents: has_sub,
                });
                // Recurse
                self.collect_subagents(child_agent, out, depth + 1);
            }
        }
    }

    fn build_subagent_node(&self, agent: &Arc<dyn IAgent>) -> SubAgentNode {
        let meta = agent.metadata();
        let mut children = Vec::new();
        for (child_id, child_agent) in &self.agents {
            if agent.get_subagent(child_id).is_some() {
                children.push(self.build_subagent_node(child_agent));
            }
        }

        SubAgentNode {
            id: agent.id().to_string(),
            name: meta.key.clone(),
            agent_type: meta.agent_type.clone(),
            description: meta.description.clone(),
            capability_tags: meta.capability_tags.clone(),
            children,
        }
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

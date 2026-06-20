//! Multi-agent registration center with sub-agent discovery.
//!
//! Stores `Arc<dyn IAgent>` instances and provides:
//! - Registration and lookup by `AgentId`
//! - Recursive sub-agent discovery via `get_subagent()`
//! - Agent list metadata for ACP `initialize` response

use std::collections::HashMap;
use std::sync::Arc;

use rust_agent_core::{AgentId, IAgent};
use serde::{Deserialize, Serialize};

/// 已注册 Agent 的信息，通过 `_raf/agent_list` 和 `initialize._meta` 暴露。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub agent_type: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tool_names: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capability_tags: Vec<String>,
    #[serde(default)]
    pub has_subagents: bool,
    /// 此 Agent 是默认的（当 session/new 中未指定 agent_id 时使用）。
    #[serde(default)]
    pub is_default: bool,
}

/// 子 Agent 的信息，通过 `_raf/subagent_list` 暴露。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentInfo {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capability_tags: Vec<String>,
    /// 在 Agent 树中的深度（0 = 查询 Agent 的直接子级）。
    pub depth: usize,
    #[serde(default)]
    pub has_subagents: bool,
}

/// `_raf/subagent_tree` 的树节点。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentNode {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub capability_tags: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<SubAgentNode>,
}

/// 多 Agent 注册表。
pub struct AgentRegistry {
    agents: HashMap<AgentId, Arc<dyn IAgent>>,
    /// 默认 Agent ID（第一个注册的 Agent）。
    default_id: Option<AgentId>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            default_id: None,
        }
    }

    /// 注册一个 Agent。
    pub fn register(&mut self, agent: Arc<dyn IAgent>) {
        let id = agent.id().clone();
        if self.default_id.is_none() {
            self.default_id = Some(id.clone());
        }
        self.agents.insert(id, agent);
    }

    /// 按 ID 查找 Agent。
    pub fn get(&self, id: &AgentId) -> Option<&Arc<dyn IAgent>> {
        self.agents.get(id)
    }

    /// 获取默认 Agent。
    pub fn get_default(&self) -> Option<&Arc<dyn IAgent>> {
        self.default_id.as_ref().and_then(|id| self.agents.get(id))
    }

    /// 返回所有已注册的 Agent ID。
    pub fn ids(&self) -> Vec<&AgentId> {
        self.agents.keys().collect()
    }

    /// 已注册 Agent 的数量。
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// 为所有已注册 Agent 构建 Agent 列表信息。
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

    /// 构建 Agent 列表作为 ACP `initialize` 响应的 `_meta` 值。
    pub fn build_agent_list_meta(&self) -> serde_json::Value {
        let agents = self.build_agent_list();
        serde_json::json!({
            "raf": {
                "agents": agents,
                "version": "0.1.0"
            }
        })
    }

    /// 通过递归调用 `get_subagent()` 获取指定 Agent 的子 Agent 列表。
    pub fn get_subagent_list(&self, agent_id: &AgentId) -> Vec<SubAgentInfo> {
        let agent = match self.agents.get(agent_id) {
            Some(a) => a,
            None => return vec![],
        };
        let mut result = Vec::new();
        self.collect_subagents(agent, &mut result, 0);
        result
    }

    /// 获取指定 Agent 的子 Agent 树。
    pub fn get_subagent_tree(&self, agent_id: &AgentId) -> Option<SubAgentNode> {
        let agent = self.agents.get(agent_id)?;
        Some(self.build_subagent_node(agent))
    }

    /// 解析请求的目标 Agent。检查 `_meta.raf.agent_id`，回退到默认。
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

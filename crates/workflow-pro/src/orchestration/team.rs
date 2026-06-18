use std::collections::HashMap;
use std::sync::Arc;
use rust_agent_core::{IAgent};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRole {
    pub name: String,
    pub description: String,
    pub capabilities: Vec<AgentCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentCapability {
    pub name: String,
    pub tags: Vec<String>,
}

pub struct AgentTeam {
    name: String,
    agents: HashMap<String, Arc<dyn IAgent>>,
    roles: HashMap<String, AgentRole>,
}

impl AgentTeam {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), agents: HashMap::new(), roles: HashMap::new() }
    }

    pub fn register_agent(&mut self, agent: Arc<dyn IAgent>, role: AgentRole) {
        let id = agent.id().to_string();
        self.agents.insert(id, agent);
        self.roles.insert(role.name.clone(), role);
    }

    pub fn find_agent_by_capability(&self, capability: &str) -> Vec<&Arc<dyn IAgent>> {
        let mut result = Vec::new();
        for (_role_name, role) in &self.roles {
            if role.capabilities.iter().any(|c| c.name == capability || c.tags.contains(&capability.to_string())) {
                for agent in self.agents.values() {
                    result.push(agent);
                }
            }
        }
        if result.is_empty() {
            result.extend(self.agents.values());
        }
        result
    }

    pub fn agents(&self) -> Vec<&Arc<dyn IAgent>> {
        self.agents.values().collect()
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use rust_agent_core::IAgent;

#[derive(Debug, Clone)]
pub struct AgentPoolConfig {
    pub min_size: usize,
    pub max_size: usize,
    pub idle_timeout: Duration,
    pub heartbeat_interval: Duration,
}

impl Default for AgentPoolConfig {
    fn default() -> Self {
        Self {
            min_size: 1,
            max_size: 10,
            idle_timeout: Duration::from_secs(300),
            heartbeat_interval: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentHealth {
    Healthy,
    Degraded,
    Unhealthy,
}

pub struct PooledAgent {
    pub agent: Arc<dyn IAgent>,
    pub last_used: Mutex<Instant>,
    pub health: Mutex<AgentHealth>,
    pub request_count: Mutex<u64>,
}

pub struct AgentPool {
    config: AgentPoolConfig,
    agents: Mutex<Vec<PooledAgent>>,
}

impl AgentPool {
    pub fn new(config: AgentPoolConfig) -> Self {
        Self { config, agents: Mutex::new(Vec::new()) }
    }

    pub fn add_agent(&self, agent: Arc<dyn IAgent>) {
        let mut agents = self.agents.lock();
        if agents.len() < self.config.max_size {
            agents.push(PooledAgent {
                agent,
                last_used: Mutex::new(Instant::now()),
                health: Mutex::new(AgentHealth::Healthy),
                request_count: Mutex::new(0),
            });
        }
    }

    pub fn acquire(&self) -> Option<Arc<dyn IAgent>> {
        let agents = self.agents.lock();
        for pooled in agents.iter() {
            let health = pooled.health.lock().clone();
            if health == AgentHealth::Healthy {
                *pooled.last_used.lock() = Instant::now();
                *pooled.request_count.lock() += 1;
                return Some(pooled.agent.clone());
            }
        }
        None
    }

    pub fn heartbeat(&self) {
        let agents = self.agents.lock();
        for pooled in agents.iter() {
            let elapsed = pooled.last_used.lock().elapsed();
            let mut health = pooled.health.lock();
            if elapsed > self.config.idle_timeout {
                *health = AgentHealth::Degraded;
            }
        }
    }

    pub fn health_status(&self) -> Vec<(String, AgentHealth)> {
        self.agents.lock().iter()
            .map(|p| (p.agent.id().to_string(), p.health.lock().clone()))
            .collect()
    }
}
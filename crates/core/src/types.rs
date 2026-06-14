use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Unique identifier for an agent instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Metadata describing an agent, following MAF's AgentMetadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub agent_type: String,
    pub key: String,
    pub description: String,
}

/// A tool call requested by the agent during response generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// The result of executing a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
}

/// Metadata for each content/event variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub agent_id: Option<AgentId>,
    pub model_id: Option<String>,
    pub executor_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub properties: HashMap<String, serde_json::Value>,
}

/// Finish reason from LLM
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    #[serde(untagged)]
    Other(String),
}

/// Usage statistics including KV cache hit/miss
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub prompt_cache_hit_tokens: Option<u32>,
    pub prompt_cache_miss_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
}

impl Usage {
    /// Compute cache hit ratio, choosing the formula appropriate for the provider.
    ///
    /// - If `prompt_cache_miss_tokens` is present (DeepSeek): `hit / (hit + miss)`
    /// - Otherwise (OpenAI): `hit / prompt_tokens`
    /// - Returns `0.0` when no cache data is available.
    pub fn cache_hit_ratio(&self) -> f64 {
        let hit = self.prompt_cache_hit_tokens.unwrap_or(0) as f64;
        if hit == 0.0 {
            return 0.0;
        }
        if let Some(miss) = self.prompt_cache_miss_tokens {
            let miss = miss as f64;
            let total = hit + miss;
            if total > 0.0 {
                return hit / total;
            }
        }
        let prompt = self.prompt_tokens as f64;
        if prompt > 0.0 {
            hit / prompt
        } else {
            0.0
        }
    }
}

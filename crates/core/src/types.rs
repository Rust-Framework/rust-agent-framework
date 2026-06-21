use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 智能体实例的唯一标识符。
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

/// 描述智能体身份与能力的静态元数据。
///
/// 由 AgentRegistry 用于动态发现，前端和编排引擎
/// 可在不调用智能体的情况下查询完整的能力矩阵。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentMetadata {
    /// 智能体类名（如 "ChatClientAgent"、"WorkflowAgent"）
    pub agent_type: String,
    /// 短标识符（智能体 ID 的字符串形式）
    pub key: String,
    /// 人类可读的描述，未设置时从指令自动生成。
    pub description: String,
    /// 已注册工具名称列表（如 ["read_file", "web_search"]）
    #[serde(default)]
    pub tool_names: Vec<String>,
    /// LLM 模型标识符（如 "agnes-2.0-flash"）
    #[serde(default)]
    pub model_id: Option<String>,
    /// 用于发现的能力标签（如 ["file_operations", "code"]）
    #[serde(default)]
    pub capability_tags: Vec<String>,
    /// 系统指令的前 200 个字符，用于快速预览
    #[serde(default)]
    pub instructions_preview: String,
}

impl AgentMetadata {
    pub fn new(agent_type: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            agent_type: agent_type.into(),
            key: key.into(),
            description: String::new(),
            tool_names: Vec::new(),
            model_id: None,
            capability_tags: Vec::new(),
            instructions_preview: String::new(),
        }
    }
}

/// 智能体在生成响应时请求的工具调用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// 每个内容/事件变体的元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub agent_id: Option<AgentId>,
    pub model_id: Option<String>,
    pub executor_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub properties: HashMap<String, serde_json::Value>,
}

/// LLM 返回的结束原因
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    /// 智能体因工具需要人工审批而暂停。
    /// 会话保留完整上下文（包括 assistant(tool_calls) 消息）。
    /// 调用方应收集审批决定并通过 `AgentRunOptions.tool_approval_responses` 恢复。
    AwaitingApproval,
    /// 工具调用循环达到最大轮次限制并被强制终止。
    /// 智能体可能希望进行更多工具调用但被截断。
    MaxRounds,
    #[serde(untagged)]
    Other(String),
}

/// 用量统计，包括 KV 缓存命中/未命中
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
    /// 计算缓存命中率，根据提供商选择合适的公式。
    ///
    /// - 如果提供了 `prompt_cache_miss_tokens`（DeepSeek）：`hit / (hit + miss)`
    /// - 否则（OpenAI）：`hit / prompt_tokens`
    /// - 无缓存数据时返回 `0.0`。
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

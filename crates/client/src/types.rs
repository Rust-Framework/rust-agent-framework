use serde::{Deserialize, Serialize};

/// Model list entry item from GET /models API.
/// Both OpenAI (`/v1/models`) and DeepSeek (`/models`) follow this format:
/// `{ "object": "list", "data": [{ "id": "...", "object": "model", ... }] }`
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelListEntry {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub owned_by: String,
}

/// Usage statistics from the chat completion response.
/// Parsed from the last chunk when `stream_options: { include_usage: true }`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct UsageStats {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// DeepSeek: cache-hit tokens in this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_hit_tokens: Option<u32>,
    /// DeepSeek: cache-miss tokens in this request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_miss_tokens: Option<u32>,
    /// DeepSeek: reasoning/thinking tokens consumed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
}

/// Aggregated cache hit information extracted from usage stats.
#[derive(Debug, Clone, Default)]
pub struct CacheHitInfo {
    pub cache_hit_tokens: u32,
    pub cache_miss_tokens: u32,
    /// cache_hit_tokens / (cache_hit_tokens + cache_miss_tokens), 0.0 if both are 0.
    pub cache_hit_ratio: f64,
}

impl From<&UsageStats> for CacheHitInfo {
    fn from(usage: &UsageStats) -> Self {
        let hit = usage.prompt_cache_hit_tokens.unwrap_or(0);
        let miss = usage.prompt_cache_miss_tokens.unwrap_or(0);
        let total = hit + miss;
        Self {
            cache_hit_tokens: hit,
            cache_miss_tokens: miss,
            cache_hit_ratio: if total > 0 { hit as f64 / total as f64 } else { 0.0 },
        }
    }
}

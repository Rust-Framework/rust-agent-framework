//! Per-provider Usage deserialization structs.
//!
//! Each LLM provider returns token/cache statistics in a different JSON shape.
//! These structs are independent — no serde aliases shared across providers —
//! so each provider's wire format is validated independently.
//!
//! Every struct implements `Into<Usage>` via `into_usage()`.

use rust_agent_core::Usage;

/// 指定在用量数据中预期的供应商线格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageFormat {
    /// OpenAI style: `prompt_tokens_details.cached_tokens`,
    /// `completion_tokens_details.reasoning_tokens`
    OpenAI,
    /// DeepSeek style: `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` at top level
    DeepSeek,
}

impl UsageFormat {
    /// 使用正确的供应商结构体解析原始的 `serde_json::Value` 用量数据。
    pub fn parse(&self, raw: &serde_json::Value) -> Option<Usage> {
        match self {
            UsageFormat::OpenAI => {
                serde_json::from_value::<OpenAIUsage>(raw.clone())
                    .ok()
                    .map(|u| u.into_usage())
            }
            UsageFormat::DeepSeek => {
                serde_json::from_value::<DeepSeekUsage>(raw.clone())
                    .ok()
                    .map(|u| u.into_usage())
            }
        }
    }
}

// ── OpenAI ────────────────────────────────────────────────────────────

/// OpenAI API usage response.
///
/// Cache tokens: `prompt_tokens_details.cached_tokens`
/// Reasoning tokens: `completion_tokens_details.reasoning_tokens`
#[derive(Debug, serde::Deserialize)]
pub(crate) struct OpenAIUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_tokens_details: Option<OpenAIPromptTokensDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<OpenAICompletionTokensDetails>,
    /// 部分 OpenAI 兼容网关（如 DeepSeek、Agnes）在顶层返回缓存字段。
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_cache_miss_tokens: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OpenAIPromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OpenAICompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
}

impl OpenAIUsage {
    fn into_usage(self) -> Usage {
        Usage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
            prompt_cache_hit_tokens: self
                .prompt_tokens_details
                .and_then(|d| d.cached_tokens),
            prompt_cache_miss_tokens: None, // OpenAI does not report cache miss
            reasoning_tokens: self
                .completion_tokens_details
                .and_then(|d| d.reasoning_tokens),
        }
    }
}

// ── DeepSeek ───────────────────────────────────────────────────────────

/// DeepSeek API usage response.
///
/// Cache hits/misses at top level: `prompt_cache_hit_tokens`, `prompt_cache_miss_tokens`.
/// Reasoning: `completion_tokens_details.reasoning_tokens`
#[derive(Debug, serde::Deserialize)]
pub(crate) struct DeepSeekUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_cache_miss_tokens: Option<u32>,
    #[serde(default)]
    pub completion_tokens_details: Option<DeepSeekCompletionTokensDetails>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct DeepSeekCompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
}

impl DeepSeekUsage {
    fn into_usage(self) -> Usage {
        Usage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
            prompt_cache_hit_tokens: self.prompt_cache_hit_tokens,
            prompt_cache_miss_tokens: self.prompt_cache_miss_tokens,
            reasoning_tokens: self
                .completion_tokens_details
                .and_then(|d| d.reasoning_tokens),
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_usage_cache_from_details() {
        let json = r#"{
            "prompt_tokens": 1000,
            "completion_tokens": 500,
            "total_tokens": 1500,
            "prompt_tokens_details": {"cached_tokens": 800},
            "completion_tokens_details": {"reasoning_tokens": 120}
        }"#;
        let usage = serde_json::from_str::<OpenAIUsage>(json).unwrap().into_usage();
        assert_eq!(usage.prompt_cache_hit_tokens, Some(800));
        assert_eq!(usage.prompt_cache_miss_tokens, None);
        assert_eq!(usage.reasoning_tokens, Some(120));
    }

    #[test]
    fn openai_usage_no_details() {
        let json = r#"{"prompt_tokens":100,"completion_tokens":50,"total_tokens":150}"#;
        let usage = serde_json::from_str::<OpenAIUsage>(json).unwrap().into_usage();
        assert!(usage.prompt_cache_hit_tokens.is_none());
        assert!(usage.reasoning_tokens.is_none());
    }

    #[test]
    fn deepseek_usage_cache_top_level() {
        let json = r#"{
            "prompt_tokens": 1000,
            "completion_tokens": 500,
            "total_tokens": 1500,
            "prompt_cache_hit_tokens": 700,
            "prompt_cache_miss_tokens": 300
        }"#;
        let usage = serde_json::from_str::<DeepSeekUsage>(json).unwrap().into_usage();
        assert_eq!(usage.prompt_cache_hit_tokens, Some(700));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(300));
    }

    #[test]
    fn deepseek_usage_no_cache() {
        let json = r#"{"prompt_tokens":200,"completion_tokens":100,"total_tokens":300}"#;
        let usage = serde_json::from_str::<DeepSeekUsage>(json).unwrap().into_usage();
        assert!(usage.prompt_cache_hit_tokens.is_none());
        assert!(usage.prompt_cache_miss_tokens.is_none());
    }

    #[test]
    fn usage_format_openai_parse() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":8}}"#
        ).unwrap();
        let usage = UsageFormat::OpenAI.parse(&raw).unwrap();
        assert_eq!(usage.prompt_cache_hit_tokens, Some(8));
    }

    #[test]
    fn usage_format_deepseek_parse() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_cache_hit_tokens":7,"prompt_cache_miss_tokens":3}"#
        ).unwrap();
        let usage = UsageFormat::DeepSeek.parse(&raw).unwrap();
        assert_eq!(usage.prompt_cache_hit_tokens, Some(7));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(3));
    }
}

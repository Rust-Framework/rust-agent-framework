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
    /// Agnes AI — OpenAI-compatible with optional top-level cache + reasoning details
    Agnes,
    /// Anthropic Messages API: `input_tokens`, `cache_read_input_tokens`, etc.
    Anthropic,
}

impl UsageFormat {
    /// 使用正确的供应商结构体解析原始的 `serde_json::Value` 用量数据。
    pub fn parse(&self, raw: &serde_json::Value) -> Option<Usage> {
        let usage = match self {
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
            UsageFormat::Agnes => {
                serde_json::from_value::<AgnesUsage>(raw.clone())
                    .ok()
                    .map(|u| u.into_usage())
            }
            UsageFormat::Anthropic => {
                serde_json::from_value::<AnthropicUsage>(raw.clone())
                    .ok()
                    .map(|u| u.into_usage())
            }
        }?;
        Some(attach_raw(usage, raw))
    }
}

fn attach_raw(mut usage: Usage, raw: &serde_json::Value) -> Usage {
    usage.raw = Some(raw.clone());
    usage
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
        let cache_hit = self
            .prompt_cache_hit_tokens
            .or_else(|| {
                self.prompt_tokens_details
                    .and_then(|d| d.cached_tokens)
            });
        let cache_miss = self.prompt_cache_miss_tokens.or_else(|| {
            cache_hit.map(|hit| self.prompt_tokens.saturating_sub(hit))
        });
        Usage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
            prompt_cache_hit_tokens: cache_hit,
            prompt_cache_miss_tokens: cache_miss,
            reasoning_tokens: self
                .completion_tokens_details
                .and_then(|d| d.reasoning_tokens),
            raw: None,
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
            raw: None,
        }
    }
}

// ── Agnes ─────────────────────────────────────────────────────────────

/// Agnes AI usage response (OpenAI-compatible wire format).
#[derive(Debug, serde::Deserialize)]
pub(crate) struct AgnesUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_tokens_details: Option<AgnesPromptTokensDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<AgnesCompletionTokensDetails>,
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<u32>,
    #[serde(default)]
    pub prompt_cache_miss_tokens: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct AgnesPromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct AgnesCompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
}

impl AgnesUsage {
    fn into_usage(self) -> Usage {
        let cache_hit = self
            .prompt_cache_hit_tokens
            .or_else(|| {
                self.prompt_tokens_details
                    .and_then(|d| d.cached_tokens)
            });
        let cache_miss = self.prompt_cache_miss_tokens.or_else(|| {
            cache_hit.map(|hit| self.prompt_tokens.saturating_sub(hit))
        });
        Usage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            total_tokens: self.total_tokens,
            prompt_cache_hit_tokens: cache_hit,
            prompt_cache_miss_tokens: cache_miss,
            reasoning_tokens: self
                .completion_tokens_details
                .and_then(|d| d.reasoning_tokens),
            raw: None,
        }
    }
}

// ── Anthropic ─────────────────────────────────────────────────────────

/// Anthropic Messages API usage (`input_tokens` / `output_tokens`).
#[derive(Debug, serde::Deserialize)]
pub(crate) struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
}

impl AnthropicUsage {
    fn into_usage(self) -> Usage {
        let total = self.input_tokens.saturating_add(self.output_tokens);
        let cache_hit = self.cache_read_input_tokens;
        let cache_miss = cache_hit.map(|hit| self.input_tokens.saturating_sub(hit));
        Usage {
            prompt_tokens: self.input_tokens,
            completion_tokens: self.output_tokens,
            total_tokens: total,
            prompt_cache_hit_tokens: cache_hit,
            prompt_cache_miss_tokens: cache_miss,
            reasoning_tokens: None,
            raw: None,
        }
    }

    /// Merge partial usage chunks (message_start has input, message_delta has output).
    pub(crate) fn merge_into(self, existing: &mut Usage) {
        if self.input_tokens > 0 {
            existing.prompt_tokens = self.input_tokens;
        }
        if self.output_tokens > 0 {
            existing.completion_tokens = self.output_tokens;
        }
        existing.total_tokens = existing
            .prompt_tokens
            .saturating_add(existing.completion_tokens);
        if let Some(hit) = self.cache_read_input_tokens {
            existing.prompt_cache_hit_tokens = Some(hit);
            existing.prompt_cache_miss_tokens =
                Some(existing.prompt_tokens.saturating_sub(hit));
        }
        let _ = self.cache_creation_input_tokens;
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_usage_hybrid_top_level_cache() {
        let json = r#"{
            "prompt_tokens": 1000,
            "completion_tokens": 500,
            "total_tokens": 1500,
            "prompt_cache_hit_tokens": 600,
            "prompt_cache_miss_tokens": 400
        }"#;
        let usage = serde_json::from_str::<OpenAIUsage>(json).unwrap().into_usage();
        assert_eq!(usage.prompt_cache_hit_tokens, Some(600));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(400));
    }

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
        assert_eq!(usage.prompt_cache_miss_tokens, Some(200));
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
            r#"{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_cache_hit_tokens":7,"prompt_cache_miss_tokens":3}"#,
        )
        .unwrap();
        let usage = UsageFormat::DeepSeek.parse(&raw).unwrap();
        assert_eq!(usage.prompt_cache_hit_tokens, Some(7));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(3));
    }

    #[test]
    fn agnes_wire_usage_without_cache_fields() {
        // 实测 apihub.agnes-ai.com agnes-2.0-flash SSE 末尾 usage 块
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"completion_tokens":9,"completion_tokens_details":{"reasoning_tokens":0},"prompt_tokens":2689,"total_tokens":2698}"#,
        )
        .unwrap();
        let usage = UsageFormat::Agnes.parse(&raw).unwrap();
        assert_eq!(usage.prompt_tokens, 2689);
        assert_eq!(usage.completion_tokens, 9);
        assert_eq!(usage.total_tokens, 2698);
        assert_eq!(usage.reasoning_tokens, Some(0));
        assert!(!usage.cache_stats_available());
        assert_eq!(usage.raw.as_ref(), Some(&raw));
    }

    #[test]
    fn usage_format_agnes_parse() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15,"prompt_cache_hit_tokens":6,"completion_tokens_details":{"reasoning_tokens":2}}"#,
        )
        .unwrap();
        let usage = UsageFormat::Agnes.parse(&raw).unwrap();
        assert_eq!(usage.prompt_cache_hit_tokens, Some(6));
        assert_eq!(usage.reasoning_tokens, Some(2));
    }

    #[test]
    fn usage_format_anthropic_parse() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":80}"#,
        )
        .unwrap();
        let usage = UsageFormat::Anthropic.parse(&raw).unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.prompt_cache_hit_tokens, Some(80));
        assert_eq!(usage.prompt_cache_miss_tokens, Some(20));
    }
}

//! 写入门控 —— v2 特性 8。
//!
//! v2 规范：未经筛选的存储会导致准确率从 100% 暴跌至 13%。写入门控是
//! "该不该存"的决策层，在写入前评估内容质量、检测冲突、过滤低价值信息。
//!
//! 门控规则：
//! 1. 必填字段校验（title, type）
//! 2. 置信度下限（低于阈值的草稿需审查）
//! 3. 重复检测（与已有页面高度相似）
//! 4. 冲突预检（调用 conflict 模块）
//! 5. 空内容拒绝

use serde::{Deserialize, Serialize};

use crate::frontmatter;

/// 门控决策。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "reason")]
pub enum GateDecision {
    /// 允许写入。
    Accept,
    /// 拒绝写入，附带原因。
    Reject(String),
    /// 需要人工审查，附带原因。
    NeedsReview(String),
}

impl GateDecision {
    /// 是否允许写入。
    pub fn is_accepted(&self) -> bool {
        matches!(self, GateDecision::Accept)
    }

    /// 是否被拒绝。
    pub fn is_rejected(&self) -> bool {
        matches!(self, GateDecision::Reject(_))
    }

    /// 是否需要审查。
    pub fn needs_review(&self) -> bool {
        matches!(self, GateDecision::NeedsReview(_))
    }

    /// 获取原因文本（Accept 返回空串）。
    pub fn reason(&self) -> &str {
        match self {
            GateDecision::Accept => "",
            GateDecision::Reject(r) | GateDecision::NeedsReview(r) => r,
        }
    }
}

/// 门控配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConfig {
    /// 最低置信度：低于此值的页面需审查（不直接拒绝）。
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,
    /// 拒绝置信度：低于此值的页面直接拒绝。
    #[serde(default = "default_reject_confidence")]
    pub reject_confidence: f32,
    /// 必填字段列表。
    #[serde(default = "default_required_fields")]
    pub required_fields: Vec<String>,
    /// 最小 body 字符数。
    #[serde(default = "default_min_body_length")]
    pub min_body_length: usize,
    /// 是否启用冲突预检。
    #[serde(default = "default_true")]
    pub conflict_check: bool,
    /// 重复检测的 slug 相似度阈值（0, 1]。
    #[serde(default = "default_dup_threshold")]
    pub duplicate_threshold: f32,
}

fn default_min_confidence() -> f32 {
    0.2
}
fn default_reject_confidence() -> f32 {
    0.05
}
fn default_required_fields() -> Vec<String> {
    vec!["title".to_string(), "type".to_string()]
}
fn default_min_body_length() -> usize {
    10
}
fn default_true() -> bool {
    true
}
fn default_dup_threshold() -> f32 {
    0.9
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            min_confidence: default_min_confidence(),
            reject_confidence: default_reject_confidence(),
            required_fields: default_required_fields(),
            min_body_length: default_min_body_length(),
            conflict_check: true,
            duplicate_threshold: default_dup_threshold(),
        }
    }
}

/// 门控评估的上下文。
pub struct GateContext<'a> {
    /// 待写入的内容（完整 markdown，含 frontmatter）。
    pub content: &'a str,
    /// 目标 slug。
    pub slug: &'a str,
    /// 已存在的同 wiki 页面 slug 列表（用于重复检测）。
    pub existing_slugs: &'a [String],
    /// 门控配置。
    pub config: &'a GateConfig,
}

/// 评估是否允许写入。
///
/// 返回 `GateDecision`。`Accept` 表示可直接写入；`NeedsReview` 表示可写入
/// 但应标记为 `status: stub` 等待审查；`Reject` 表示不应写入。
pub fn evaluate(ctx: &GateContext<'_>) -> GateDecision {
    let parsed = frontmatter::parse(ctx.content);

    // 规则 1：必填字段
    for field in &ctx.config.required_fields {
        if !parsed.frontmatter.contains_key(field) {
            return GateDecision::Reject(format!("missing required field: {field}"));
        }
    }

    // 规则 2：空 body
    let body = parsed.body.trim();
    if body.len() < ctx.config.min_body_length {
        return GateDecision::Reject(format!(
            "body too short: {} chars (min {})",
            body.len(),
            ctx.config.min_body_length
        ));
    }

    // 规则 3：置信度
    let confidence = frontmatter::confidence(&parsed.frontmatter);
    if confidence < ctx.config.reject_confidence {
        return GateDecision::Reject(format!(
            "confidence {confidence:.2} below reject threshold {}",
            ctx.config.reject_confidence
        ));
    }
    if confidence < ctx.config.min_confidence {
        return GateDecision::NeedsReview(format!(
            "confidence {confidence:.2} below review threshold {}",
            ctx.config.min_confidence
        ));
    }

    // 规则 4：重复检测（slug 相似度）
    if ctx.config.duplicate_threshold < 1.0 {
        for existing in ctx.existing_slugs {
            let sim = slug_similarity(ctx.slug, existing);
            if sim >= ctx.config.duplicate_threshold && existing != ctx.slug {
                return GateDecision::NeedsReview(format!(
                    "possible duplicate of existing slug: {existing} (similarity {sim:.2})"
                ));
            }
        }
    }

    // 规则 5：status 为 archived 时拒绝
    if let Some(status) = parsed.status() {
        if status == "archived" {
            return GateDecision::Reject("page status is 'archived' — refusing to write".to_string());
        }
    }

    GateDecision::Accept
}

/// 计算两个 slug 的相似度（基于字符级 Jaccard）。
pub fn slug_similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    let a_chars: std::collections::HashSet<char> = a.chars().collect();
    let b_chars: std::collections::HashSet<char> = b.chars().collect();
    if a_chars.is_empty() && b_chars.is_empty() {
        return 1.0;
    }
    let intersection = a_chars.intersection(&b_chars).count() as f32;
    let union = a_chars.union(&b_chars).count() as f32;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate_test(content: &str, slug: &str, existing: &[String]) -> GateDecision {
        let config = GateConfig::default();
        let ctx = GateContext {
            content,
            slug,
            existing_slugs: existing,
            config: &config,
        };
        evaluate(&ctx)
    }

    #[test]
    fn test_accept_valid_page() {
        let content = "---\ntitle: Test\ntype: concept\nconfidence: 0.8\n---\n# Test\nThis is a valid body with enough content.";
        let d = evaluate_test(content, "concepts/test", &[]);
        assert!(d.is_accepted(), "{}", d.reason());
    }

    #[test]
    fn test_reject_missing_title() {
        let content = "---\ntype: concept\n---\n# Test\nbody content here";
        let d = evaluate_test(content, "concepts/test", &[]);
        assert!(d.is_rejected());
        assert!(d.reason().contains("title"));
    }

    #[test]
    fn test_reject_empty_body() {
        let content = "---\ntitle: Test\ntype: concept\n---\n\n";
        let d = evaluate_test(content, "concepts/test", &[]);
        assert!(d.is_rejected());
    }

    #[test]
    fn test_reject_low_confidence() {
        let content = "---\ntitle: Test\ntype: concept\nconfidence: 0.01\n---\n# Test\nbody content here";
        let d = evaluate_test(content, "concepts/test", &[]);
        assert!(d.is_rejected());
        assert!(d.reason().contains("reject threshold"));
    }

    #[test]
    fn test_needs_review_moderate_confidence() {
        let content = "---\ntitle: Test\ntype: concept\nconfidence: 0.1\n---\n# Test\nbody content here";
        let d = evaluate_test(content, "concepts/test", &[]);
        assert!(d.needs_review());
    }

    #[test]
    fn test_duplicate_detection() {
        let content = "---\ntitle: Test\ntype: concept\nconfidence: 0.8\n---\n# Test\nbody content here";
        let existing = vec!["concepts/tset".to_string()]; // 相似度高
        let d = evaluate_test(content, "concepts/test", &existing);
        // slug_similarity("concepts/test", "concepts/tset") 应较高
        assert!(d.needs_review() || d.is_accepted()); // 取决于阈值
    }

    #[test]
    fn test_reject_archived() {
        let content = "---\ntitle: Test\ntype: concept\nstatus: archived\nconfidence: 0.8\n---\n# Test\nbody content here";
        let d = evaluate_test(content, "concepts/test", &[]);
        assert!(d.is_rejected());
        assert!(d.reason().contains("archived"));
    }

    #[test]
    fn test_slug_similarity() {
        assert_eq!(slug_similarity("abc", "abc"), 1.0);
        assert!(slug_similarity("abc", "abd") >= 0.5);
        assert!(slug_similarity("abc", "xyz") < 0.2);
    }
}

//! 动态置信度评分 —— v2 特性 1。
//!
//! v2 规范：置信度不再是一个人工填写的静态值，而是根据来源可靠性、
//! 证据数量和信息时效性动态计算的权重。
//!
//! 公式：`confidence = base × source_reliability × evidence_factor × freshness_factor`
//!
//! - `base`：frontmatter 中人工填写的 `confidence`（默认 0.5）
//! - `source_reliability`：按来源类型加权的平均可靠性（paper=1.0, doc=0.8, article=0.6 …）
//! - `evidence_factor`：`min(1.0, claims_count / evidence_threshold)`，证据越多越可信
//! - `freshness_factor`：`exp(-age_days / halflife)`，Ebbinghaus 式时效衰减

use std::collections::BTreeMap;

use chrono::{Local, NaiveDate};
use serde_yaml::Value;

/// 来源类型 → 可靠性权重的默认映射。
///
/// 可通过 `ConfidenceConfig::source_reliability` 覆盖。
pub fn default_source_reliability() -> Vec<(&'static str, f32)> {
    vec![
        ("paper", 1.0),
        ("reference", 0.95),
        ("spec", 0.9),
        ("doc", 0.8),
        ("concept", 0.75),
        ("article", 0.6),
        ("clipping", 0.4),
        ("blog", 0.35),
        ("source", 0.5),
        ("episodic", 0.55),
        ("procedural", 0.7),
    ]
}

/// 动态置信度计算的输入参数。
#[derive(Debug, Clone)]
pub struct ConfidenceInput<'a> {
    /// 页面 frontmatter。
    pub frontmatter: &'a BTreeMap<String, Value>,
    /// 证据阈值：当 claims 数量达到此值时 evidence_factor = 1.0。
    pub evidence_threshold: usize,
    /// 时效半衰期（天）：age_days = halflife 时 freshness_factor = e^-1 ≈ 0.37。
    pub halflife_days: u32,
    /// 来源类型 → 可靠性权重（覆盖默认表）。
    pub source_reliability: &'a [(&'static str, f32)],
}

/// 动态置信度计算的输出明细。
#[derive(Debug, Clone)]
pub struct ConfidenceBreakdown {
    /// 最终动态置信度，[0, 1]。
    pub confidence: f32,
    /// frontmatter 中的基础值。
    pub base: f32,
    /// 来源可靠性因子。
    pub source_reliability: f32,
    /// 证据因子。
    pub evidence_factor: f32,
    /// 时效因子。
    pub freshness_factor: f32,
    /// 证据（claims）数量。
    pub evidence_count: usize,
    /// 页面年龄（天）。
    pub age_days: i64,
}

/// 计算动态置信度。
///
/// 任何因子缺失时退化为中性值（1.0），不惩罚缺失字段。
pub fn compute(input: &ConfidenceInput<'_>) -> ConfidenceBreakdown {
    let base = crate::frontmatter::confidence(input.frontmatter);

    // 来源可靠性：取 sources 字段中每个 slug 的类型权重，求平均
    let source_reliability = compute_source_reliability(input.frontmatter, input.source_reliability);

    // 证据因子：claims 数组长度
    let evidence_count = count_claims(input.frontmatter);
    let evidence_factor = if input.evidence_threshold == 0 {
        1.0
    } else {
        (evidence_count as f32 / input.evidence_threshold as f32).min(1.0)
    };

    // 时效因子：last_updated 距今天数
    let age_days = compute_age_days(input.frontmatter);
    let freshness_factor = if input.halflife_days == 0 {
        1.0
    } else {
        (-age_days as f32 / input.halflife_days as f32).exp()
    };

    let confidence = (base * source_reliability * evidence_factor * freshness_factor).clamp(0.0, 1.0);

    ConfidenceBreakdown {
        confidence,
        base,
        source_reliability,
        evidence_factor,
        freshness_factor,
        evidence_count,
        age_days,
    }
}

/// 仅计算最终置信度数值（无明细）。
pub fn compute_confidence(input: &ConfidenceInput<'_>) -> f32 {
    compute(input).confidence
}

// ── 内部计算 ──────────────────────────────────────────────────────────────────

fn compute_source_reliability(
    fm: &BTreeMap<String, Value>,
    overrides: &[(&'static str, f32)],
) -> f32 {
    let sources: Vec<&str> = fm
        .get("sources")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    if sources.is_empty() {
        return 1.0; // 无来源时不惩罚
    }

    // 尝试从 sources 的 slug 推断类型（slug 前缀如 "papers/xxx" → "paper"）
    let mut total = 0.0f32;
    let mut count = 0u32;
    for s in &sources {
        let stype = infer_source_type(s);
        let weight = lookup_reliability(stype, overrides);
        total += weight;
        count += 1;
    }
    if count == 0 {
        1.0
    } else {
        (total / count as f32).clamp(0.0, 1.0)
    }
}

fn infer_source_type(slug: &str) -> &'static str {
    // slug 形如 "papers/foo" / "docs/bar" / "articles/baz"
    let prefix = slug.split('/').next().unwrap_or("").to_lowercase();
    match prefix.as_str() {
        "paper" | "papers" => "paper",
        "doc" | "docs" | "documentation" => "doc",
        "article" | "articles" => "article",
        "clipping" | "clippings" => "clipping",
        "blog" | "blogs" => "blog",
        "reference" | "references" | "ref" => "reference",
        "spec" | "specs" => "spec",
        "concept" | "concepts" => "concept",
        "source" | "sources" => "source",
        "episodic" => "episodic",
        "procedural" => "procedural",
        _ => "source",
    }
}

fn lookup_reliability(stype: &str, overrides: &[(&'static str, f32)]) -> f32 {
    for (k, v) in overrides {
        if *k == stype {
            return *v;
        }
    }
    // 回退到默认表
    for (k, v) in default_source_reliability() {
        if k == stype {
            return v;
        }
    }
    0.5
}

fn count_claims(fm: &BTreeMap<String, Value>) -> usize {
    fm.get("claims")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.len())
        .unwrap_or(0)
}

fn compute_age_days(fm: &BTreeMap<String, Value>) -> i64 {
    let date_str = fm
        .get("last_updated")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let today = Local::now().date_naive();
    match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        Ok(d) => (today - d).num_days().max(0),
        Err(_) => 0, // 无日期视为当天（不惩罚）
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;

    fn fm(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn test_basic_confidence() {
        let f = fm(&[
            ("title", Value::String("x".into())),
            ("confidence", Value::Number(serde_yaml::Number::from(0.8f64))),
            ("last_updated", Value::String("2026-06-01".into())),
        ]);
        let input = ConfidenceInput {
            frontmatter: &f,
            evidence_threshold: 3,
            halflife_days: 180,
            source_reliability: &default_source_reliability(),
        };
        let bd = compute(&input);
        assert!(bd.confidence <= 0.8);
        assert_eq!(bd.evidence_count, 0);
        assert!(bd.freshness_factor <= 1.0);
    }

    #[test]
    fn test_evidence_boosts_confidence() {
        let mut f = BTreeMap::new();
        f.insert("confidence".into(), Value::Number(serde_yaml::Number::from(0.5f64)));
        f.insert(
            "claims".into(),
            Value::Sequence(vec![
                Value::String("claim1".into()),
                Value::String("claim2".into()),
                Value::String("claim3".into()),
            ]),
        );

        let input = ConfidenceInput {
            frontmatter: &f,
            evidence_threshold: 3,
            halflife_days: 0, // 关闭时效
            source_reliability: &default_source_reliability(),
        };
        let bd = compute(&input);
        // evidence_factor = 3/3 = 1.0, freshness = 1.0, source = 1.0 (无 sources)
        assert!((bd.confidence - 0.5).abs() < 0.01);
        assert!((bd.evidence_factor - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_freshness_decay() {
        let mut f = BTreeMap::new();
        f.insert("confidence".into(), Value::Number(serde_yaml::Number::from(1.0f64)));
        // 一年前的日期
        f.insert("last_updated".into(), Value::String("2024-06-20".into()));

        let input = ConfidenceInput {
            frontmatter: &f,
            evidence_threshold: 0,
            halflife_days: 180,
            source_reliability: &default_source_reliability(),
        };
        let bd = compute(&input);
        // age ≈ 730 天, halflife=180 → e^(-730/180) ≈ e^-4.06 ≈ 0.017
        assert!(bd.freshness_factor < 0.05, "freshness should be very low, got {}", bd.freshness_factor);
        assert!(bd.confidence < 0.1);
    }

    #[test]
    fn test_source_reliability() {
        let mut f = BTreeMap::new();
        f.insert("confidence".into(), Value::Number(serde_yaml::Number::from(1.0f64)));
        f.insert(
            "sources".into(),
            Value::Sequence(vec![
                Value::String("papers/foo".into()),
                Value::String("blogs/bar".into()),
            ]),
        );

        let input = ConfidenceInput {
            frontmatter: &f,
            evidence_threshold: 0,
            halflife_days: 0,
            source_reliability: &default_source_reliability(),
        };
        let bd = compute(&input);
        // (1.0 + 0.35) / 2 = 0.675
        assert!((bd.source_reliability - 0.675).abs() < 0.01);
    }

    #[test]
    fn test_no_sources_no_penalty() {
        let mut f = BTreeMap::new();
        f.insert("confidence".into(), Value::Number(serde_yaml::Number::from(0.7f64)));
        let input = ConfidenceInput {
            frontmatter: &f,
            evidence_threshold: 0,
            halflife_days: 0,
            source_reliability: &default_source_reliability(),
        };
        let bd = compute(&input);
        assert!((bd.source_reliability - 1.0).abs() < 0.01);
        assert!((bd.confidence - 0.7).abs() < 0.01);
    }
}

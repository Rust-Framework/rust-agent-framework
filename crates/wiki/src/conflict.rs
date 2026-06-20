//! 冲突解决 —— v2 特性 7。
//!
//! v2 规范：当新旧信息矛盾时，AI 不再只是标注，而是根据权威度和时效性
//! 主动提议解决方案。
//!
//! 冲突检测策略：
//! 1. 显式 `conflicts-with` / `contradicts` 图边声明
//! 2. 同主题多源矛盾识别（同 slug 前缀 + 相反 claims）
//! 3. `superseded_by` 链中的循环检测
//!
//! 解决提议基于：来源权威度 + 时效性 + 证据数量。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::confidence;
use crate::forgetting;

/// 一条检测到的冲突。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    /// 冲突主题（通常是 slug 前缀或共同标签）。
    pub topic: String,
    /// 涉及的页面 slug 列表。
    pub pages: Vec<String>,
    /// 冲突类型。
    pub kind: ConflictKind,
    /// 冲突描述。
    pub description: String,
    /// 提议的解决方案。
    pub resolution: Resolution,
}

/// 冲突类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictKind {
    /// 显式 conflicts-with 边。
    ExplicitContradiction,
    /// 同主题相反声明。
    OpposingClaims,
    /// superseded_by 循环。
    SupersedeCycle,
    /// 重复内容（高相似度但不同 slug）。
    Duplicate,
}

/// 冲突解决提议。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// 建议的动作。
    pub action: ResolutionAction,
    /// 推荐保留的页面 slug（若有）。
    pub preferred_page: Option<String>,
    /// 推荐归档/删除的页面 slug 列表。
    pub deprecated_pages: Vec<String>,
    /// 理由。
    pub rationale: String,
}

/// 解决动作类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResolutionAction {
    /// 保留权威页面，归档其他。
    KeepAuthoritative,
    /// 合并内容。
    Merge,
    /// 需要人工裁决。
    ManualReview,
    /// 标记为 superseded。
    Supersede,
}

/// 冲突检测的上下文。
pub struct ConflictContext<'a> {
    /// 待检测的新页面 frontmatter。
    pub frontmatter: &'a BTreeMap<String, Value>,
    /// 新页面 slug。
    pub slug: &'a str,
    /// 同 wiki 的其他页面（slug, frontmatter）列表。
    pub existing_pages: &'a [(String, BTreeMap<String, Value>)],
    /// 置信度计算输入（用于权威评分）。
    pub confidence_input: &'a confidence::ConfidenceInput<'a>,
    /// 衰减配置（用于时效评分）。
    pub decay_config: &'a forgetting::DecayConfig,
}

/// 检测新页面与已有页面之间的冲突。
pub fn detect(ctx: &ConflictContext<'_>) -> Vec<Conflict> {
    let mut conflicts = Vec::new();

    // 1. 显式 conflicts-with 声明
    conflicts.extend(detect_explicit_conflicts(ctx));

    // 2. 同主题相反声明
    conflicts.extend(detect_opposing_claims(ctx));

    // 3. superseded_by 循环
    conflicts.extend(detect_supersede_cycles(ctx));

    conflicts
}

/// 为已检测到的冲突生成解决提议。
///
/// 基于权威度（confidence breakdown）+ 时效性（decay retention）选择保留页面。
pub fn propose_resolution(
    conflict: &Conflict,
    page_scores: &[(String, f32)], // (slug, authority_score)
) -> Resolution {
    if conflict.kind == ConflictKind::SupersedeCycle {
        return Resolution {
            action: ResolutionAction::ManualReview,
            preferred_page: None,
            deprecated_pages: vec![],
            rationale: "superseded_by 循环无法自动解决，需人工裁决".to_string(),
        };
    }

    // 按 authority_score 降序排列
    let mut ranked: Vec<&(String, f32)> = page_scores
        .iter()
        .filter(|(s, _)| conflict.pages.contains(s))
        .collect();
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if ranked.is_empty() {
        return Resolution {
            action: ResolutionAction::ManualReview,
            preferred_page: None,
            deprecated_pages: vec![],
            rationale: "无可用权威评分".to_string(),
        };
    }

    let preferred = ranked[0].0.clone();
    let deprecated: Vec<String> = ranked[1..]
        .iter()
        .map(|(s, _)| s.clone())
        .collect();

    let action = if conflict.kind == ConflictKind::Duplicate {
        ResolutionAction::Supersede
    } else {
        ResolutionAction::KeepAuthoritative
    };

    Resolution {
        action,
        preferred_page: Some(preferred.clone()),
        deprecated_pages: deprecated.clone(),
        rationale: format!(
            "保留权威度最高的页面 {} (score {:.2})，其余 {} 个页面建议归档",
            preferred,
            ranked[0].1,
            deprecated.len()
        ),
    }
}

/// 计算页面的权威度分数：动态置信度 × 衰减保留率。
pub fn authority_score(
    fm: &BTreeMap<String, Value>,
    confidence_input: &confidence::ConfidenceInput<'_>,
    decay_config: &forgetting::DecayConfig,
) -> f32 {
    let cb = confidence::compute(confidence_input);
    let dr = forgetting::decay_from_frontmatter(fm, decay_config);
    (cb.confidence * dr.retention).clamp(0.0, 1.0)
}

// ── 内部检测器 ────────────────────────────────────────────────────────────────

fn detect_explicit_conflicts(ctx: &ConflictContext<'_>) -> Vec<Conflict> {
    let mut out = Vec::new();
    let conflicts_with: Vec<&str> = ctx
        .frontmatter
        .get("conflicts_with")
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    for target in conflicts_with {
        let exists = ctx.existing_pages.iter().any(|(s, _)| s == target);
        if exists {
            out.push(Conflict {
                topic: ctx.slug.to_string(),
                pages: vec![ctx.slug.to_string(), target.to_string()],
                kind: ConflictKind::ExplicitContradiction,
                description: format!("{} 显式声明与 {} 冲突", ctx.slug, target),
                resolution: Resolution {
                    action: ResolutionAction::ManualReview,
                    preferred_page: None,
                    deprecated_pages: vec![],
                    rationale: "待 propose_resolution 评分".to_string(),
                },
            });
        }
    }
    out
}

fn detect_opposing_claims(ctx: &ConflictContext<'_>) -> Vec<Conflict> {
    let mut out = Vec::new();
    let my_claims = extract_claim_texts(ctx.frontmatter);
    if my_claims.is_empty() {
        return out;
    }

    let my_topic = slug_topic(ctx.slug);

    for (other_slug, other_fm) in ctx.existing_pages {
        if other_slug == ctx.slug {
            continue;
        }
        let other_topic = slug_topic(other_slug);
        // 同主题（slug 前缀匹配）
        if my_topic != other_topic || my_topic.is_empty() {
            continue;
        }
        let other_claims = extract_claim_texts(other_fm);
        if other_claims.is_empty() {
            continue;
        }
        // 检测相反声明（简单启发式：包含 "not" / "不" 的相反表述）
        for mine in &my_claims {
            for theirs in &other_claims {
                if claims_contradict(mine, theirs) {
                    out.push(Conflict {
                        topic: my_topic.clone(),
                        pages: vec![ctx.slug.to_string(), other_slug.clone()],
                        kind: ConflictKind::OpposingClaims,
                        description: format!(
                            "声明冲突:\n  {} → {}\n  {} → {}",
                            ctx.slug, mine, other_slug, theirs
                        ),
                        resolution: Resolution {
                            action: ResolutionAction::ManualReview,
                            preferred_page: None,
                            deprecated_pages: vec![],
                            rationale: "待 propose_resolution 评分".to_string(),
                        },
                    });
                    break;
                }
            }
        }
    }
    out
}

fn detect_supersede_cycles(ctx: &ConflictContext<'_>) -> Vec<Conflict> {
    let mut out = Vec::new();
    let my_supersedes: Vec<&str> = ctx
        .frontmatter
        .get("superseded_by")
        .and_then(|v| v.as_str())
        .into_iter()
        .collect();

    for target in my_supersedes {
        // 检查 target 是否也 superseded_by 我
        if let Some((_, target_fm)) = ctx.existing_pages.iter().find(|(s, _)| s == target) {
            let target_supersedes = target_fm
                .get("superseded_by")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if target_supersedes == ctx.slug {
                out.push(Conflict {
                    topic: ctx.slug.to_string(),
                    pages: vec![ctx.slug.to_string(), target.to_string()],
                    kind: ConflictKind::SupersedeCycle,
                    description: format!(
                        "superseded_by 循环: {} ↔ {}",
                        ctx.slug, target
                    ),
                    resolution: Resolution {
                        action: ResolutionAction::ManualReview,
                        preferred_page: None,
                        deprecated_pages: vec![],
                        rationale: "循环无法自动解决".to_string(),
                    },
                });
            }
        }
    }
    out
}

fn extract_claim_texts(fm: &BTreeMap<String, Value>) -> Vec<String> {
    fm.get("claims")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|c| {
                    c.as_mapping()
                        .and_then(|m| m.get("text"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_lowercase())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn slug_topic(slug: &str) -> String {
    slug.split('/').next().unwrap_or("").to_lowercase()
}

fn claims_contradict(a: &str, b: &str) -> bool {
    // 简单启发式：一方包含 "not"/"不"/"无"，另一方包含相同关键词但不含否定
    let a_neg = a.contains("not ") || a.contains("don't") || a.contains("不") || a.contains("无") || a.contains("非");
    let b_neg = b.contains("not ") || b.contains("don't") || b.contains("不") || b.contains("无") || b.contains("非");
    if a_neg == b_neg {
        return false;
    }
    // 提取关键词（简单分词）
    let a_words: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let b_words: std::collections::HashSet<&str> = b.split_whitespace().collect();
    let common = a_words.intersection(&b_words).count();
    // 至少 2 个共同词（排除否定词）
    common >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn test_explicit_conflict_detection() {
        let new_fm = fm(&[
            ("title", Value::String("A".into())),
            ("conflicts_with", Value::Sequence(vec![Value::String("concepts/b".into())])),
        ]);
        let existing = vec![
            ("concepts/b".to_string(), fm(&[("title", Value::String("B".into()))])),
        ];
        let ci = confidence::ConfidenceInput {
            frontmatter: &new_fm,
            evidence_threshold: 0,
            halflife_days: 0,
            source_reliability: &confidence::default_source_reliability(),
        };
        let ctx = ConflictContext {
            frontmatter: &new_fm,
            slug: "concepts/a",
            existing_pages: &existing,
            confidence_input: &ci,
            decay_config: &forgetting::DecayConfig::default(),
        };
        let conflicts = detect(&ctx);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].kind, ConflictKind::ExplicitContradiction);
    }

    #[test]
    fn test_supersede_cycle() {
        let new_fm = fm(&[
            ("title", Value::String("A".into())),
            ("superseded_by", Value::String("concepts/b".into())),
        ]);
        let existing = vec![
            ("concepts/b".to_string(), fm(&[
                ("title", Value::String("B".into())),
                ("superseded_by", Value::String("concepts/a".into())),
            ])),
        ];
        let ci = confidence::ConfidenceInput {
            frontmatter: &new_fm,
            evidence_threshold: 0,
            halflife_days: 0,
            source_reliability: &confidence::default_source_reliability(),
        };
        let ctx = ConflictContext {
            frontmatter: &new_fm,
            slug: "concepts/a",
            existing_pages: &existing,
            confidence_input: &ci,
            decay_config: &forgetting::DecayConfig::default(),
        };
        let conflicts = detect(&ctx);
        assert!(conflicts.iter().any(|c| c.kind == ConflictKind::SupersedeCycle));
    }

    #[test]
    fn test_propose_resolution_keep_authoritative() {
        let conflict = Conflict {
            topic: "x".into(),
            pages: vec!["a".into(), "b".into()],
            kind: ConflictKind::ExplicitContradiction,
            description: "test".into(),
            resolution: Resolution {
                action: ResolutionAction::ManualReview,
                preferred_page: None,
                deprecated_pages: vec![],
                rationale: "".into(),
            },
        };
        let scores = vec![("a".to_string(), 0.9), ("b".to_string(), 0.3)];
        let res = propose_resolution(&conflict, &scores);
        assert_eq!(res.action, ResolutionAction::KeepAuthoritative);
        assert_eq!(res.preferred_page, Some("a".to_string()));
        assert_eq!(res.deprecated_pages, vec!["b".to_string()]);
    }

    #[test]
    fn test_claims_contradict() {
        assert!(claims_contradict("rust is not memory safe", "rust is memory safe"));
        assert!(!claims_contradict("rust is fast", "rust is safe"));
    }
}

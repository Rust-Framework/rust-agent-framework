//! 遗忘曲线 —— v2 特性 5。
//!
//! v2 规范：长期未被访问或强化的事实会逐渐降权并淡出。架构决策衰减慢，
//! 临时 Bug 衰减快。
//!
//! 实现 Ebbinghaus 式指数衰减：`retention = exp(-age / halflife)`，
//! 按页面类型配置不同的半衰期。

use std::collections::BTreeMap;
use std::collections::HashMap;

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

/// 按类型配置的半衰期（天）。
///
/// key 为页面 type，value 为半衰期天数。未命中的类型使用 `default_halflife_days`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayConfig {
    /// 默认半衰期（天）。
    #[serde(default = "default_halflife")]
    pub default_halflife_days: u32,
    /// 按类型覆盖的半衰期表。
    #[serde(default)]
    pub halflife_by_type: HashMap<String, u32>,
    /// 衰减后的置信度低于此阈值时，页面被标记为"可遗忘"。
    #[serde(default = "default_forget_threshold")]
    pub forget_threshold: f32,
    /// 衰减后的置信度低于此阈值时，页面被标记为"可归档"。
    #[serde(default = "default_archive_threshold")]
    pub archive_threshold: f32,
}

fn default_halflife() -> u32 {
    90
}
fn default_forget_threshold() -> f32 {
    0.2
}
fn default_archive_threshold() -> f32 {
    0.05
}

impl Default for DecayConfig {
    fn default() -> Self {
        let mut halflife_by_type = HashMap::new();
        // 架构决策慢衰减
        halflife_by_type.insert("concept".to_string(), 365);
        halflife_by_type.insert("spec".to_string(), 365);
        halflife_by_type.insert("reference".to_string(), 365);
        // 文档中等衰减
        halflife_by_type.insert("doc".to_string(), 180);
        halflife_by_type.insert("paper".to_string(), 365);
        halflife_by_type.insert("procedural".to_string(), 180);
        // 临时内容快衰减
        halflife_by_type.insert("bug".to_string(), 30);
        halflife_by_type.insert("clipping".to_string(), 60);
        halflife_by_type.insert("article".to_string(), 90);
        halflife_by_type.insert("episodic".to_string(), 60);

        Self {
            default_halflife_days: 90,
            halflife_by_type,
            forget_threshold: 0.2,
            archive_threshold: 0.05,
        }
    }
}

/// 衰减计算结果。
#[derive(Debug, Clone)]
pub struct DecayResult {
    /// 衰减后的置信度。
    pub decayed_confidence: f32,
    /// 原始置信度。
    pub original_confidence: f32,
    /// 保留率（0, 1]。
    pub retention: f32,
    /// 页面年龄（天）。
    pub age_days: i64,
    /// 适用的半衰期（天）。
    pub halflife_days: u32,
    /// 衰减状态分类。
    pub status: DecayStatus,
}

/// 衰减后的状态分类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecayStatus {
    /// 保留率充足，无需动作。
    Fresh,
    /// 保留率低，建议强化（重新访问/更新）。
    Fading,
    /// 置信度低于 forget_threshold，可遗忘。
    Forgettable,
    /// 置信度低于 archive_threshold，建议归档。
    Archivable,
}

/// 计算单个页面的衰减。
///
/// `base_confidence` 为 frontmatter 中的原始 confidence；
/// `page_type` 用于查半衰期表；`last_updated` 为 YYYY-MM-DD 字符串。
pub fn decay(
    base_confidence: f32,
    page_type: &str,
    last_updated: &str,
    config: &DecayConfig,
) -> DecayResult {
    let halflife = config
        .halflife_by_type
        .get(page_type)
        .copied()
        .unwrap_or(config.default_halflife_days);

    let age_days = parse_age_days(last_updated);
    let retention = if halflife == 0 {
        1.0
    } else {
        (-age_days as f32 / halflife as f32).exp()
    };
    let decayed = (base_confidence * retention).clamp(0.0, 1.0);

    let status = if decayed <= config.archive_threshold {
        DecayStatus::Archivable
    } else if decayed <= config.forget_threshold {
        DecayStatus::Forgettable
    } else if retention < 0.5 {
        DecayStatus::Fading
    } else {
        DecayStatus::Fresh
    };

    DecayResult {
        decayed_confidence: decayed,
        original_confidence: base_confidence,
        retention,
        age_days,
        halflife_days: halflife,
        status,
    }
}

/// 从 frontmatter 计算衰减。
pub fn decay_from_frontmatter(
    fm: &BTreeMap<String, Value>,
    config: &DecayConfig,
) -> DecayResult {
    let base = crate::frontmatter::confidence(fm);
    let page_type = fm
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("page");
    let last_updated = fm
        .get("last_updated")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    decay(base, page_type, last_updated, config)
}

/// 强化页面：将 last_updated 更新为今天，重置衰减。
///
/// 返回需要写入 frontmatter 的 (key, value) 对。
pub fn reinforce(fm: &BTreeMap<String, Value>) -> Vec<(String, Value)> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let mut updates = vec![("last_updated".to_string(), Value::String(today))];
    // 若有 access_count 字段，递增
    if let Some(v) = fm.get("access_count") {
        if let Some(n) = v.as_u64() {
            updates.push((
                "access_count".to_string(),
                Value::Number(serde_yaml::Number::from(n + 1)),
            ));
        }
    }
    updates
}

fn parse_age_days(date_str: &str) -> i64 {
    let today = Local::now().date_naive();
    match NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        Ok(d) => (today - d).num_days().max(0),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_page() {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let r = decay(0.9, "concept", &today, &DecayConfig::default());
        assert_eq!(r.status, DecayStatus::Fresh);
        assert!((r.retention - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_old_concept_slow_decay() {
        // concept 半衰期 365 天，2 年前 → e^(-730/365) = e^-2 ≈ 0.135
        let r = decay(1.0, "concept", "2024-06-20", &DecayConfig::default());
        assert!(r.retention < 0.2, "retention = {}", r.retention);
        assert!(r.retention > 0.05);
        // 1.0 * 0.135 = 0.135 > 0.05 (archive) but < 0.2 (forget)
        assert_eq!(r.status, DecayStatus::Forgettable);
    }

    #[test]
    fn test_old_bug_fast_decay() {
        // bug 半衰期 30 天，半年前 → e^(-180/30) = e^-6 ≈ 0.0025
        let r = decay(1.0, "bug", "2025-12-20", &DecayConfig::default());
        assert!(r.retention < 0.01);
        assert_eq!(r.status, DecayStatus::Archivable);
    }

    #[test]
    fn test_unknown_type_uses_default() {
        let r = decay(0.5, "unknowntype", "2026-06-20", &DecayConfig::default());
        assert_eq!(r.halflife_days, 90);
        assert_eq!(r.status, DecayStatus::Fresh);
    }

    #[test]
    fn test_reinforce_resets_age() {
        let mut fm = BTreeMap::new();
        fm.insert("access_count".into(), Value::Number(serde_yaml::Number::from(5u64)));
        let updates = reinforce(&fm);
        assert!(updates.iter().any(|(k, _)| k == "last_updated"));
        assert!(updates.iter().any(|(k, _)| k == "access_count"));
    }
}

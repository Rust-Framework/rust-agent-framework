//! 分层记忆架构 —— v2 特性 2。
//!
//! v2 规范：仿生学的分层记忆架构，类似人类大脑的存储结构：
//! - **工作记忆**（Working）：处理即时观察，会话级，内存中。
//! - **情节记忆**（Episodic）：压缩后的会话摘要，沉淀为 `episodic` 类型页面。
//! - **语义记忆**（Semantic）：跨会话的硬核事实，沉淀为 `concept` 类型页面（高置信度）。
//! - **程序性记忆**（Procedural）：沉淀下来的工作流与模式，沉淀为 `skill` 类型页面。
//!
//! 层级越高，知识越精炼，存放时间也越长。迁移引擎负责在层间提升：
//! working → episodic（压缩）→ semantic（验证后提升）→ procedural（模式提取）。

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// 记忆层级标签。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryTier {
    Working,
    Episodic,
    Semantic,
    Procedural,
}

impl MemoryTier {
    /// 对应的 wiki 页面 type。
    pub fn page_type(&self) -> &'static str {
        match self {
            MemoryTier::Working => "working", // 不落盘，仅内存
            MemoryTier::Episodic => "episodic",
            MemoryTier::Semantic => "concept",
            MemoryTier::Procedural => "skill",
        }
    }

    /// 默认半衰期（天）。
    pub fn default_halflife(&self) -> u32 {
        match self {
            MemoryTier::Working => 1,     // 即时，1 天
            MemoryTier::Episodic => 60,   // 2 个月
            MemoryTier::Semantic => 365,  // 1 年
            MemoryTier::Procedural => 180, // 半年
        }
    }
}

/// 一条工作记忆观察。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// 观察内容。
    pub content: String,
    /// 来源（如 "session:abc", "tool:wiki_search"）。
    pub source: String,
    /// 时间戳（Unix 秒）。
    pub timestamp: i64,
    /// 关联的 slug（若有）。
    pub related_slug: Option<String>,
}

/// 一条情节记忆（会话摘要）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicEntry {
    /// 摘要内容。
    pub summary: String,
    /// 会话 ID。
    pub session_id: String,
    /// 时间范围 (start, end) Unix 秒。
    pub time_range: (i64, i64),
    /// 涉及的关键 slug。
    pub key_slugs: Vec<String>,
    /// 沉淀为 wiki 页面的 slug（若有）。
    pub page_slug: Option<String>,
}

/// 一条语义记忆（硬事实）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEntry {
    /// 事实陈述。
    pub fact: String,
    /// 支持证据（来源 slug 列表）。
    pub evidence: Vec<String>,
    /// 置信度。
    pub confidence: f32,
    /// 沉淀为 wiki 页面的 slug。
    pub page_slug: String,
}

/// 一条程序性记忆（工作流模式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProceduralEntry {
    /// 模式名称。
    pub name: String,
    /// 工作流步骤描述。
    pub steps: Vec<String>,
    /// 适用场景。
    pub context: String,
    /// 沉淀为 wiki 页面的 slug。
    pub page_slug: String,
}

/// 分层记忆系统 —— 管理四层记忆的存储与迁移。
pub struct MemoryStore {
    /// 工作记忆：会话级观察缓冲（内存，不落盘）。
    working: Mutex<Vec<Observation>>,
    /// 情节记忆：会话摘要（内存 + 可落盘为 episodic 页面）。
    episodic: Mutex<Vec<EpisodicEntry>>,
    /// 语义记忆：硬事实索引（slug → entry）。
    semantic: Mutex<HashMap<String, SemanticEntry>>,
    /// 程序性记忆：工作流模式（slug → entry）。
    procedural: Mutex<HashMap<String, ProceduralEntry>>,
    /// 配置。
    config: MemoryConfig,
}

/// 记忆系统配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// 工作记忆最大容量（超出触发压缩）。
    #[serde(default = "default_working_capacity")]
    pub working_capacity: usize,
    /// 压缩阈值：工作记忆条数达到此值时触发 → episodic 迁移。
    #[serde(default = "default_compress_threshold")]
    pub compress_threshold: usize,
    /// 情节记忆提升为语义记忆的最小置信度。
    #[serde(default = "default_promote_confidence")]
    pub promote_confidence: f32,
}

fn default_working_capacity() -> usize {
    100
}
fn default_compress_threshold() -> usize {
    20
}
fn default_promote_confidence() -> f32 {
    0.7
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            working_capacity: default_working_capacity(),
            compress_threshold: default_compress_threshold(),
            promote_confidence: default_promote_confidence(),
        }
    }
}

impl MemoryStore {
    /// 创建一个空的记忆系统。
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            working: Mutex::new(Vec::new()),
            episodic: Mutex::new(Vec::new()),
            semantic: Mutex::new(HashMap::new()),
            procedural: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// 记录一条工作记忆观察。
    pub fn observe(&self, obs: Observation) {
        let mut w = self.working.lock();
        w.push(obs);
        // 超容量时丢弃最旧的
        if w.len() > self.config.working_capacity {
            w.remove(0);
        }
    }

    /// 返回当前工作记忆的快照。
    pub fn working_snapshot(&self) -> Vec<Observation> {
        self.working.lock().clone()
    }

    /// 返回情节记忆条目。
    pub fn episodic_entries(&self) -> Vec<EpisodicEntry> {
        self.episodic.lock().clone()
    }

    /// 返回语义记忆条目。
    pub fn semantic_entries(&self) -> Vec<SemanticEntry> {
        self.semantic.lock().values().cloned().collect()
    }

    /// 返回程序性记忆条目。
    pub fn procedural_entries(&self) -> Vec<ProceduralEntry> {
        self.procedural.lock().values().cloned().collect()
    }

    /// 压缩工作记忆为情节记忆。
    ///
    /// 将工作记忆中的观察按 session_id 分组，生成摘要。
    /// `summarize` 函数负责实际的摘要生成（通常由 LLM 完成）。
    pub fn compress<F>(&self, summarize: F) -> Vec<EpisodicEntry>
    where
        F: Fn(&[Observation]) -> String,
    {
        let mut w = self.working.lock();
        if w.len() < self.config.compress_threshold {
            return Vec::new();
        }

        // 按 session 分组
        let mut by_session: HashMap<String, Vec<Observation>> = HashMap::new();
        for obs in w.drain(..) {
            by_session
                .entry(obs.source.clone())
                .or_default()
                .push(obs);
        }

        let mut new_entries = Vec::new();
        let mut e = self.episodic.lock();
        for (session, observations) in by_session {
            if observations.is_empty() {
                continue;
            }
            let timestamps: Vec<i64> = observations.iter().map(|o| o.timestamp).collect();
            let start = *timestamps.iter().min().unwrap_or(&0);
            let end = *timestamps.iter().max().unwrap_or(&0);
            let key_slugs: Vec<String> = observations
                .iter()
                .filter_map(|o| o.related_slug.clone())
                .collect();
            let summary = summarize(&observations);
            let entry = EpisodicEntry {
                summary,
                session_id: session,
                time_range: (start, end),
                key_slugs,
                page_slug: None,
            };
            new_entries.push(entry.clone());
            e.push(entry);
        }
        new_entries
    }

    /// 将情节记忆提升为语义记忆。
    ///
    /// `extract_facts` 函数负责从摘要中提取硬事实（通常由 LLM 完成）。
    /// 提取的事实需达到 `promote_confidence` 才会被提升。
    pub fn promote<F>(&self, extract_facts: F) -> Vec<SemanticEntry>
    where
        F: Fn(&EpisodicEntry) -> Vec<(String, f32)>, // (fact, confidence)
    {
        let mut promoted = Vec::new();
        let e = self.episodic.lock();
        let mut s = self.semantic.lock();
        for entry in e.iter() {
            let facts = extract_facts(entry);
            for (fact, conf) in facts {
                if conf >= self.config.promote_confidence {
                    let slug = format!("semantic/{}", slugify(&fact));
                    let sem = SemanticEntry {
                        fact,
                        evidence: entry.key_slugs.clone(),
                        confidence: conf,
                        page_slug: slug.clone(),
                    };
                    s.insert(slug, sem.clone());
                    promoted.push(sem);
                }
            }
        }
        promoted
    }

    /// 沉淀程序性记忆（工作流模式）。
    pub fn sediment_procedural(&self, entry: ProceduralEntry) {
        self.procedural.lock().insert(entry.page_slug.clone(), entry);
    }

    /// 清空工作记忆（会话结束时调用）。
    pub fn clear_working(&self) {
        self.working.lock().clear();
    }

    /// 返回各层的条目数统计。
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            working: self.working.lock().len(),
            episodic: self.episodic.lock().len(),
            semantic: self.semantic.lock().len(),
            procedural: self.procedural.lock().len(),
        }
    }
}

/// 记忆系统统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub working: usize,
    pub episodic: usize,
    pub semantic: usize,
    pub procedural: usize,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.stats();
        f.debug_struct("MemoryStore")
            .field("working", &stats.working)
            .field("episodic", &stats.episodic)
            .field("semantic", &stats.semantic)
            .field("procedural", &stats.procedural)
            .finish()
    }
}

fn slugify(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '/' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(content: &str, source: &str) -> Observation {
        Observation {
            content: content.into(),
            source: source.into(),
            timestamp: 1000,
            related_slug: Some("concepts/test".into()),
        }
    }

    #[test]
    fn test_observe_and_snapshot() {
        let ms = MemoryStore::new(MemoryConfig::default());
        ms.observe(obs("hello", "session:a"));
        ms.observe(obs("world", "session:a"));
        let snap = ms.working_snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn test_working_capacity_limit() {
        let config = MemoryConfig {
            working_capacity: 3,
            ..Default::default()
        };
        let ms = MemoryStore::new(config);
        for i in 0..5 {
            ms.observe(obs(&format!("obs {i}"), "s"));
        }
        assert_eq!(ms.working_snapshot().len(), 3);
    }

    #[test]
    fn test_compress() {
        let config = MemoryConfig {
            compress_threshold: 2,
            ..Default::default()
        };
        let ms = MemoryStore::new(config);
        ms.observe(obs("a", "session:1"));
        ms.observe(obs("b", "session:1"));
        ms.observe(obs("c", "session:2"));

        let entries = ms.compress(|obs| format!("{} observations", obs.len()));
        assert!(!entries.is_empty());
        assert_eq!(ms.working_snapshot().len(), 0); // 已清空
    }

    #[test]
    fn test_promote() {
        let config = MemoryConfig {
            compress_threshold: 1,
            promote_confidence: 0.5,
            ..Default::default()
        };
        let ms = MemoryStore::new(config);
        ms.observe(obs("rust is safe", "s"));
        ms.compress(|_| "session about rust safety".into());
        let promoted = ms.promote(|_| vec![("rust is memory safe".into(), 0.9), ("rust is slow".into(), 0.2)]);
        assert_eq!(promoted.len(), 1); // 只有 0.9 的通过
        assert!(promoted[0].fact.contains("memory safe"));
    }

    #[test]
    fn test_sediment_procedural() {
        let ms = MemoryStore::new(MemoryConfig::default());
        ms.sediment_procedural(ProceduralEntry {
            name: "review code".into(),
            steps: vec!["read".into(), "comment".into()],
            context: "PR review".into(),
            page_slug: "skills/code-review".into(),
        });
        assert_eq!(ms.procedural_entries().len(), 1);
    }
}

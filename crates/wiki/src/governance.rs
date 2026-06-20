//! 自动化治理钩子 —— v2 特性 6。
//!
//! v2 规范：自动化钩子自动摄取新源、自动压缩会话、定期清理冗余。
//! 将繁琐的"维护工作"彻底交给 AI 代理。
//!
//! 本模块提供：
//! - 定时任务调度器（基于 tokio interval）
//! - 三类治理任务：auto_ingest / compress_sessions / cleanup_redundancy
//! - 与 watch.rs 的 WatchAction 协同

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::forgetting::{DecayConfig, DecayStatus};
use crate::memory::MemoryStore;

/// 治理任务类型。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GovernanceTask {
    /// 自动摄取外部源（URL/RSS）。
    AutoIngestSources,
    /// 压缩会话为情节记忆。
    CompressSessions,
    /// 清理冗余页面（低置信度 + 高年龄）。
    CleanupRedundancy,
    /// 重建索引。
    RebuildIndex,
}

/// 治理配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    /// 调度间隔（秒）。
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,
    /// 启用的任务列表。
    #[serde(default = "default_enabled_tasks")]
    pub enabled_tasks: Vec<GovernanceTask>,
    /// 冗余清理的置信度阈值。
    #[serde(default = "default_redundancy_threshold")]
    pub redundancy_confidence_threshold: f32,
    /// 冗余清理的年龄阈值（天）。
    #[serde(default = "default_redundancy_age_days")]
    pub redundancy_age_days: u32,
    /// 自动摄取的源 URL 列表。
    #[serde(default)]
    pub ingest_sources: Vec<String>,
}

fn default_interval_secs() -> u64 {
    3600 // 1 小时
}
fn default_enabled_tasks() -> Vec<GovernanceTask> {
    vec![
        GovernanceTask::CompressSessions,
        GovernanceTask::CleanupRedundancy,
    ]
}
fn default_redundancy_threshold() -> f32 {
    0.1
}
fn default_redundancy_age_days() -> u32 {
    180
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            interval_secs: default_interval_secs(),
            enabled_tasks: default_enabled_tasks(),
            redundancy_confidence_threshold: default_redundancy_threshold(),
            redundancy_age_days: default_redundancy_age_days(),
            ingest_sources: Vec::new(),
        }
    }
}

/// 治理任务执行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceReport {
    /// 执行的任务。
    pub task: GovernanceTask,
    /// 是否成功。
    pub success: bool,
    /// 处理的条目数。
    pub items_processed: usize,
    /// 摘要信息。
    pub summary: String,
    /// 错误信息（若有）。
    pub error: Option<String>,
}

/// 治理调度器 —— 定期执行配置的任务。
pub struct GovernanceScheduler {
    config: GovernanceConfig,
    memory: Option<Arc<MemoryStore>>,
    decay_config: DecayConfig,
    reports: Mutex<Vec<GovernanceReport>>,
}

impl GovernanceScheduler {
    /// 创建调度器。
    pub fn new(config: GovernanceConfig, decay_config: DecayConfig) -> Self {
        Self {
            config,
            memory: None,
            decay_config,
            reports: Mutex::new(Vec::new()),
        }
    }

    /// 关联记忆系统（启用 CompressSessions 任务）。
    pub fn with_memory(mut self, memory: Arc<MemoryStore>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// 执行一次所有启用的任务。
    ///
    /// `scan_pages` 回调返回所有页面的 (slug, frontmatter) 列表，用于冗余清理。
    /// `archive_page` 回调用于归档页面。
    pub fn run_once<F, A>(&self, scan_pages: F, archive_page: A) -> Vec<GovernanceReport>
    where
        F: Fn() -> Vec<(String, std::collections::BTreeMap<String, serde_yaml::Value>)>,
        A: Fn(&str) -> anyhow::Result<()>,
    {
        let mut reports = Vec::new();
        for task in &self.config.enabled_tasks {
            let report = match task {
                GovernanceTask::CompressSessions => self.run_compress_sessions(),
                GovernanceTask::CleanupRedundancy => {
                    self.run_cleanup_redundancy(&scan_pages, &archive_page)
                }
                GovernanceTask::AutoIngestSources => self.run_auto_ingest(),
                GovernanceTask::RebuildIndex => self.run_rebuild_index(),
            };
            reports.push(report);
        }
        let mut r = self.reports.lock();
        r.extend(reports.clone());
        // 保留最近 100 条报告
        if r.len() > 100 {
            let drop = r.len() - 100;
            r.drain(0..drop);
        }
        reports
    }

    /// 返回历史报告。
    pub fn history(&self) -> Vec<GovernanceReport> {
        self.reports.lock().clone()
    }

    /// 返回调度间隔。
    pub fn interval(&self) -> Duration {
        Duration::from_secs(self.config.interval_secs)
    }

    // ── 内部任务实现 ──

    fn run_compress_sessions(&self) -> GovernanceReport {
        if let Some(memory) = &self.memory {
            let entries = memory.compress(|obs| {
                let contents: Vec<&str> = obs.iter().map(|o| o.content.as_str()).collect();
                format!("Session summary: {}", contents.join("; "))
            });
            GovernanceReport {
                task: GovernanceTask::CompressSessions,
                success: true,
                items_processed: entries.len(),
                summary: format!("compressed {} sessions into episodic memory", entries.len()),
                error: None,
            }
        } else {
            GovernanceReport {
                task: GovernanceTask::CompressSessions,
                success: false,
                items_processed: 0,
                summary: "no memory store attached".into(),
                error: Some("memory store not configured".into()),
            }
        }
    }

    fn run_cleanup_redundancy<F, A>(&self, scan_pages: F, archive_page: A) -> GovernanceReport
    where
        F: Fn() -> Vec<(String, std::collections::BTreeMap<String, serde_yaml::Value>)>,
        A: Fn(&str) -> anyhow::Result<()>,
    {
        let pages = scan_pages();
        let mut archived = 0usize;
        let mut errors = 0usize;
        for (slug, fm) in pages {
            let decay = crate::forgetting::decay_from_frontmatter(&fm, &self.decay_config);
            if decay.status == DecayStatus::Archivable {
                if let Err(e) = archive_page(&slug) {
                    tracing::warn!(slug = %slug, error = %e, "failed to archive page");
                    errors += 1;
                } else {
                    archived += 1;
                }
            }
        }
        GovernanceReport {
            task: GovernanceTask::CleanupRedundancy,
            success: errors == 0,
            items_processed: archived,
            summary: format!("archived {} low-confidence pages", archived),
            error: if errors > 0 {
                Some(format!("{} pages failed to archive", errors))
            } else {
                None
            },
        }
    }

    fn run_auto_ingest(&self) -> GovernanceReport {
        // 实际摄取由外部 Agent 工具调用完成，此处仅返回待摄取源列表
        GovernanceReport {
            task: GovernanceTask::AutoIngestSources,
            success: true,
            items_processed: self.config.ingest_sources.len(),
            summary: format!(
                "queued {} external sources for ingestion",
                self.config.ingest_sources.len()
            ),
            error: None,
        }
    }

    fn run_rebuild_index(&self) -> GovernanceReport {
        // 实际重建由 WikiEngine::rebuild_index 完成
        GovernanceReport {
            task: GovernanceTask::RebuildIndex,
            success: true,
            items_processed: 0,
            summary: "index rebuild requested".into(),
            error: None,
        }
    }
}

impl std::fmt::Debug for GovernanceScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GovernanceScheduler")
            .field("config", &self.config)
            .field("has_memory", &self.memory.is_some())
            .field("history_count", &self.reports.lock().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_no_memory() {
        let s = GovernanceScheduler::new(GovernanceConfig::default(), DecayConfig::default());
        let reports = s.run_once(|| vec![], |_| Ok(()));
        // CompressSessions 应失败（无 memory）
        let compress = reports.iter().find(|r| r.task == GovernanceTask::CompressSessions);
        assert!(compress.unwrap().error.is_some());
    }

    #[test]
    fn test_scheduler_with_memory() {
        let memory = Arc::new(MemoryStore::new(crate::memory::MemoryConfig {
            compress_threshold: 1,
            ..Default::default()
        }));
        memory.observe(crate::memory::Observation {
            content: "test".into(),
            source: "s".into(),
            timestamp: 0,
            related_slug: None,
        });
        let s = GovernanceScheduler::new(GovernanceConfig::default(), DecayConfig::default())
            .with_memory(memory);
        let reports = s.run_once(|| vec![], |_| Ok(()));
        let compress = reports.iter().find(|r| r.task == GovernanceTask::CompressSessions);
        assert!(compress.unwrap().success);
    }

    #[test]
    fn test_cleanup_redundancy() {
        let config = GovernanceConfig {
            enabled_tasks: vec![GovernanceTask::CleanupRedundancy],
            ..Default::default()
        };
        let s = GovernanceScheduler::new(config, DecayConfig::default());

        let mut fm = std::collections::BTreeMap::new();
        fm.insert(
            "confidence".to_string(),
            serde_yaml::Value::Number(serde_yaml::Number::from(0.01f64)),
        );
        fm.insert(
            "type".to_string(),
            serde_yaml::Value::String("bug".into()),
        );
        fm.insert(
            "last_updated".to_string(),
            serde_yaml::Value::String("2020-01-01".into()),
        );

        let reports = s.run_once(
            || vec![("bugs/old".to_string(), fm.clone())],
            |_| Ok(()),
        );
        let cleanup = reports.iter().find(|r| r.task == GovernanceTask::CleanupRedundancy);
        assert!(cleanup.unwrap().items_processed > 0);
    }
}

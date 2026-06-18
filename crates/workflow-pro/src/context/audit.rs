use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// 审计级别。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditLevel {
    Info,
    Warning,
    Error,
    Critical,
}

/// 审计条目 —— 记录工作流执行中的一个事件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub process_id: String,
    pub node_id: Option<String>,
    pub level: AuditLevel,
    pub category: String,
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: Option<u64>,
}

impl AuditEntry {
    pub fn new(
        process_id: impl Into<String>,
        level: AuditLevel,
        category: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            process_id: process_id.into(),
            node_id: None,
            level,
            category: category.into(),
            message: message.into(),
            data: None,
            timestamp: Utc::now(),
            duration_ms: None,
        }
    }

    pub fn with_node(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration_ms = Some(duration.as_millis() as u64);
        self
    }
}

/// 审计追踪 —— 收集并持久化流程执行审计事件。
///
/// 消费 `WorkflowEvent` 流，将关键事件转换为 `AuditEntry` 记录。
pub struct AuditTrail {
    entries: Mutex<Vec<AuditEntry>>,
    max_entries: usize,
}

impl AuditTrail {
    /// 创建审计追踪，指定最大保留条目数。
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(Vec::with_capacity(max_entries)),
            max_entries,
        }
    }

    /// 记录一条审计条目。
    pub fn record(&self, entry: AuditEntry) {
        let mut entries = self.entries.lock();
        if entries.len() >= self.max_entries {
            entries.remove(0);
        }
        entries.push(entry);
    }

    /// 便捷方法：记录 Info 级别事件。
    pub fn info(
        &self,
        process_id: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.record(AuditEntry::new(process_id, AuditLevel::Info, category, message));
    }

    /// 便捷方法：记录 Error 级别事件。
    pub fn error(
        &self,
        process_id: impl Into<String>,
        category: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.record(AuditEntry::new(process_id, AuditLevel::Error, category, message));
    }

    /// 查询指定流程的所有审计条目。
    pub fn entries_for(&self, process_id: &str) -> Vec<AuditEntry> {
        let entries = self.entries.lock();
        entries
            .iter()
            .filter(|e| e.process_id == process_id)
            .cloned()
            .collect()
    }

    /// 获取所有审计条目。
    pub fn all(&self) -> Vec<AuditEntry> {
        self.entries.lock().clone()
    }

    /// 清空审计追踪。
    pub fn clear(&self) {
        self.entries.lock().clear();
    }
}

impl Default for AuditTrail {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_trail_record() {
        let trail = AuditTrail::new(10);
        trail.info("proc-1", "node_exec", "Node started");
        trail.error("proc-1", "node_exec", "Node failed");

        let entries = trail.entries_for("proc-1");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, AuditLevel::Info);
        assert_eq!(entries[1].level, AuditLevel::Error);
    }

    #[test]
    fn test_audit_trail_max_entries() {
        let trail = AuditTrail::new(3);
        for i in 0..5 {
            trail.info(&format!("proc-{}", i), "test", "entry");
        }

        let all = trail.all();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_audit_trail_filter_by_process() {
        let trail = AuditTrail::new(10);
        trail.info("proc-a", "test", "A1");
        trail.info("proc-b", "test", "B1");
        trail.info("proc-a", "test", "A2");

        assert_eq!(trail.entries_for("proc-a").len(), 2);
        assert_eq!(trail.entries_for("proc-b").len(), 1);
    }
}

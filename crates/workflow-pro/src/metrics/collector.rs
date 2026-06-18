use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// 流程指标采集器 —— 收集流程实例的执行指标。
#[derive(Debug)]
pub struct ProcessMetricsCollector {
    /// 流程实例 ID → 指标
    metrics: Mutex<HashMap<String, ProcessMetrics>>,
}

/// 单个流程实例的指标。
#[derive(Debug, Clone)]
pub struct ProcessMetrics {
    pub process_id: String,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub total_nodes: u64,
    pub completed_nodes: u64,
    pub failed_nodes: u64,
    pub retried_nodes: u64,
    pub total_duration_ms: u64,
    pub node_durations: Vec<(String, u64)>, // (node_id, duration_ms)
}

impl ProcessMetrics {
    pub fn new(process_id: impl Into<String>) -> Self {
        Self {
            process_id: process_id.into(),
            started_at: Instant::now(),
            completed_at: None,
            total_nodes: 0,
            completed_nodes: 0,
            failed_nodes: 0,
            retried_nodes: 0,
            total_duration_ms: 0,
            node_durations: Vec::new(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.completed_at.is_some()
    }

    pub fn duration_ms(&self) -> u64 {
        match self.completed_at {
            Some(end) => end.duration_since(self.started_at).as_millis() as u64,
            None => self.started_at.elapsed().as_millis() as u64,
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_nodes == 0 {
            return 1.0;
        }
        self.completed_nodes as f64 / self.total_nodes as f64
    }
}

impl ProcessMetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Mutex::new(HashMap::new()),
        }
    }

    /// 注册一个新的流程实例。
    pub fn register(&self, process_id: impl Into<String>) {
        let pid = process_id.into();
        let mut metrics = self.metrics.lock();
        metrics.entry(pid.clone()).or_insert_with(|| ProcessMetrics::new(pid));
    }

    /// 记录节点开始。
    pub fn node_started(&self, process_id: &str, _node_id: &str) {
        let mut metrics = self.metrics.lock();
        if let Some(m) = metrics.get_mut(process_id) {
            m.total_nodes += 1;
        }
    }

    /// 记录节点完成。
    pub fn node_completed(&self, process_id: &str, node_id: &str, duration: Duration) {
        let mut metrics = self.metrics.lock();
        if let Some(m) = metrics.get_mut(process_id) {
            m.completed_nodes += 1;
            m.node_durations.push((node_id.to_string(), duration.as_millis() as u64));
        }
    }

    /// 记录节点失败。
    pub fn node_failed(&self, process_id: &str, _node_id: &str) {
        let mut metrics = self.metrics.lock();
        if let Some(m) = metrics.get_mut(process_id) {
            m.failed_nodes += 1;
        }
    }

    /// 记录节点重试。
    pub fn node_retried(&self, process_id: &str) {
        let mut metrics = self.metrics.lock();
        if let Some(m) = metrics.get_mut(process_id) {
            m.retried_nodes += 1;
        }
    }

    /// 标记流程完成。
    pub fn process_completed(&self, process_id: &str) {
        let mut metrics = self.metrics.lock();
        if let Some(m) = metrics.get_mut(process_id) {
            m.completed_at = Some(Instant::now());
            m.total_duration_ms = m.duration_ms();
        }
    }

    /// 获取指定流程的指标。
    pub fn get(&self, process_id: &str) -> Option<ProcessMetrics> {
        self.metrics.lock().get(process_id).cloned()
    }

    /// 获取所有流程指标。
    pub fn all(&self) -> Vec<ProcessMetrics> {
        self.metrics.lock().values().cloned().collect()
    }
}

impl Default for ProcessMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector() {
        let collector = ProcessMetricsCollector::new();
        collector.register("proc-1");
        collector.node_started("proc-1", "node-a");
        collector.node_started("proc-1", "node-b");
        collector.node_completed("proc-1", "node-a", Duration::from_millis(100));
        collector.node_failed("proc-1", "node-b");
        collector.node_retried("proc-1");
        collector.process_completed("proc-1");

        let metrics = collector.get("proc-1").unwrap();
        assert_eq!(metrics.total_nodes, 2);
        assert_eq!(metrics.completed_nodes, 1);
        assert_eq!(metrics.failed_nodes, 1);
        assert_eq!(metrics.retried_nodes, 1);
        assert!(metrics.is_complete());
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// SLA 截止时间定义。
#[derive(Debug, Clone)]
pub struct SlaDeadline {
    pub name: String,
    pub description: Option<String>,
    pub max_duration: Duration,
    pub node_id: Option<String>,
}

impl SlaDeadline {
    pub fn new(name: impl Into<String>, max_duration: Duration) -> Self {
        Self {
            name: name.into(),
            description: None,
            max_duration,
            node_id: None,
        }
    }

    pub fn for_node(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// SLA 状态。
#[derive(Debug, Clone, PartialEq)]
pub enum SlaStatus {
    /// 未开始
    Pending,
    /// 进行中（未超出 SLA）
    OnTrack,
    /// 接近 SLA 截止（超过 80%）
    AtRisk,
    /// 超出 SLA
    Breached,
    /// 在 SLA 内完成
    Met,
}

/// SLA 追踪器 —— 追踪流程节点和流程级别的 SLA 截止时间。
pub struct SlaTracker {
    deadlines: Vec<SlaDeadline>,
    /// deadline name → (started_at, status)
    tracking: Mutex<HashMap<String, (Instant, SlaStatus)>>,
    on_breach: Mutex<Option<Arc<dyn Fn(&str) + Send + Sync>>>,
}

impl SlaTracker {
    /// 创建 SLA 追踪器。
    pub fn new(deadlines: Vec<SlaDeadline>) -> Self {
        Self {
            deadlines,
            tracking: Mutex::new(HashMap::new()),
            on_breach: Mutex::new(None),
        }
    }

    /// 设置 SLA 违约回调。
    pub fn on_breach<F: Fn(&str) + Send + Sync + 'static>(&self, callback: F) {
        *self.on_breach.lock() = Some(Arc::new(callback));
    }

    /// 启动所有 SLA 截止时间的计时。
    pub fn start_all(&self) {
        let mut tracking = self.tracking.lock();
        for deadline in &self.deadlines {
            tracking.insert(deadline.name.clone(), (Instant::now(), SlaStatus::OnTrack));
        }
    }

    /// 启动指定 SLA 截止时间。
    pub fn start(&self, deadline_name: &str) {
        let mut tracking = self.tracking.lock();
        tracking.insert(deadline_name.to_string(), (Instant::now(), SlaStatus::OnTrack));
    }

    /// 完成指定 SLA 截止时间。
    pub fn complete(&self, deadline_name: &str) -> Option<SlaStatus> {
        let mut tracking = self.tracking.lock();
        if let Some((started, status)) = tracking.get_mut(deadline_name) {
            if let Some(deadline) = self.deadlines.iter().find(|d| d.name == deadline_name) {
                let elapsed = started.elapsed();
                let final_status = if elapsed > deadline.max_duration {
                    SlaStatus::Breached
                } else {
                    SlaStatus::Met
                };
                *status = final_status.clone();
                return Some(final_status);
            }
        }
        None
    }

    /// 检查所有 SLA 状态（由外部定时调用）。
    pub fn check_all(&self) -> Vec<(String, SlaStatus)> {
        let mut tracking = self.tracking.lock();
        let mut results = Vec::new();

        for deadline in &self.deadlines {
            if let Some((started, status)) = tracking.get_mut(&deadline.name) {
                let elapsed = started.elapsed();
                let ratio = elapsed.as_secs_f64() / deadline.max_duration.as_secs_f64();

                let new_status = if elapsed > deadline.max_duration {
                    if *status != SlaStatus::Breached {
                        if let Some(ref callback) = *self.on_breach.lock() {
                            callback(&deadline.name);
                        }
                    }
                    SlaStatus::Breached
                } else if ratio > 0.8 {
                    SlaStatus::AtRisk
                } else {
                    SlaStatus::OnTrack
                };

                *status = new_status.clone();
                results.push((deadline.name.clone(), new_status));
            } else {
                results.push((deadline.name.clone(), SlaStatus::Pending));
            }
        }

        results
    }

    /// 获取指定截止时间的剩余时间。
    pub fn remaining(&self, deadline_name: &str) -> Option<Duration> {
        let tracking = self.tracking.lock();
        let deadline = self.deadlines.iter().find(|d| d.name == deadline_name)?;

        if let Some((started, _)) = tracking.get(deadline_name) {
            let elapsed = started.elapsed();
            if elapsed >= deadline.max_duration {
                Some(Duration::ZERO)
            } else {
                Some(deadline.max_duration - elapsed)
            }
        } else {
            Some(deadline.max_duration)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sla_on_track() {
        let tracker = SlaTracker::new(vec![
            SlaDeadline::new("overall", Duration::from_secs(60)),
        ]);
        tracker.start_all();

        let statuses = tracker.check_all();
        assert_eq!(statuses[0].1, SlaStatus::OnTrack);
    }

    #[test]
    fn test_sla_complete_met() {
        let tracker = SlaTracker::new(vec![
            SlaDeadline::new("quick", Duration::from_millis(100)),
        ]);
        tracker.start_all();
        std::thread::sleep(Duration::from_millis(10));

        let status = tracker.complete("quick").unwrap();
        assert_eq!(status, SlaStatus::Met);
    }

    #[test]
    fn test_sla_complete_breached() {
        let tracker = SlaTracker::new(vec![
            SlaDeadline::new("slow", Duration::from_millis(10)),
        ]);
        tracker.start_all();
        std::thread::sleep(Duration::from_millis(20));

        let status = tracker.complete("slow").unwrap();
        assert_eq!(status, SlaStatus::Breached);
    }

    #[test]
    fn test_sla_remaining() {
        let tracker = SlaTracker::new(vec![
            SlaDeadline::new("overall", Duration::from_secs(10)),
        ]);
        tracker.start_all();

        let remaining = tracker.remaining("overall").unwrap();
        assert!(remaining <= Duration::from_secs(10));
        assert!(remaining > Duration::from_secs(9));
    }
}

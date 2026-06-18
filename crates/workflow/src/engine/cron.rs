use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{Datelike, Timelike};
use parking_lot::Mutex;
use rust_agent_core::Result;
use tokio::sync::mpsc::UnboundedSender;

use crate::executor::{HandlerResult, IExecutor, NodeProgress, TypeTag};
use crate::engine::IWorkflowContext;

/// Cron 表达式调度执行器 —— 基于类 cron 表达式的周期性触发。
///
/// 作为 IExecutor 插入工作流图中，每次 handle() 计算下次触发时间
/// 并注册定时器。定时器到期后 engin 重新 enqueue 消息到此节点，
/// 节点将消息传递给下游并重新调度下次触发。
///
/// 支持的 cron 格式（6 段）：
/// `秒 分 时 日 月 星期`（0=周日, 1-6=周一至周六）
///
/// 通配符 `*` 表示任意，`*/N` 表示每隔 N。
pub struct CronTrigger {
    node_id: String,
    expression: String,
    timer_name: String,
    is_fired: Mutex<bool>,
    iteration: Mutex<u64>,
    max_iterations: u64,
}

/// Cron 字段解析结果
#[derive(Debug, Clone, PartialEq)]
enum CronField {
    Any,
    Specific(Vec<u32>),
    Interval(u32),
}

impl CronTrigger {
    pub fn new(node_id: impl Into<String>, expression: impl Into<String>) -> Self {
        let id: String = node_id.into();
        Self {
            timer_name: format!("cron_{}", id),
            node_id: id,
            expression: expression.into(),
            is_fired: Mutex::new(false),
            iteration: Mutex::new(0),
            max_iterations: 0,
        }
    }

    pub fn with_max_iterations(mut self, max: u64) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_timer_name(mut self, name: impl Into<String>) -> Self {
        self.timer_name = name.into();
        self
    }

    fn parse_expression(expr: &str) -> Result<[CronField; 6]> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 6 {
            return Err(rust_agent_core::AgentError::WorkflowError(format!(
                "Cron 表达式需要 6 个字段，实际收到 {} 个: '{}'",
                parts.len(), expr
            )));
        }
        let mut fields: Vec<CronField> = Vec::with_capacity(6);
        for part in &parts {
            fields.push(Self::parse_field(part)?);
        }
        Ok([
            fields[0].clone(), fields[1].clone(), fields[2].clone(),
            fields[3].clone(), fields[4].clone(), fields[5].clone(),
        ])
    }

    fn parse_field(s: &str) -> Result<CronField> {
        if s == "*" {
            return Ok(CronField::Any);
        }
        if s.starts_with("*/") {
            let interval: u32 = s[2..].parse().map_err(|e| {
                rust_agent_core::AgentError::WorkflowError(format!(
                    "无效的 cron 间隔 '{}': {}", s, e
                ))
            })?;
            if interval == 0 {
                return Err(rust_agent_core::AgentError::WorkflowError("cron 间隔必须 > 0".into()));
            }
            return Ok(CronField::Interval(interval));
        }
        let values: Result<Vec<u32>> = s.split(',').map(|v| {
            v.trim().parse::<u32>().map_err(|e| {
                rust_agent_core::AgentError::WorkflowError(format!("无效的 cron 值 '{}': {}", v, e))
            })
        }).collect();
        Ok(CronField::Specific(values?))
    }

    /// 计算距离下次触发的时间（基于当前 UTC 时间）。
    /// 按秒迭代搜索，最坏 O(31M)。TODO: 优化为按边界跳跃。
    fn next_delay(&self) -> Result<Duration> {
        let fields = Self::parse_expression(&self.expression)?;
        let now = chrono::Utc::now();
        let (sec, min, hour, day, month, weekday) = (
            &fields[0], &fields[1], &fields[2], &fields[3], &fields[4], &fields[5],
        );
        let mut candidate = now + chrono::Duration::seconds(1);
        let max_lookahead = now + chrono::Duration::days(366);

        while candidate <= max_lookahead {
            let weekday_num = candidate.format("%u").to_string().parse::<u32>().unwrap_or(0);
            let cron_weekday = if weekday_num == 7 { 0 } else { weekday_num };
            if !Self::field_matches(sec, candidate.second())
                || !Self::field_matches(min, candidate.minute())
                || !Self::field_matches(hour, candidate.hour())
                || !Self::field_matches(day, candidate.day())
                || !Self::field_matches(month, candidate.month())
                || !Self::field_matches(weekday, cron_weekday)
            {
                candidate += chrono::Duration::seconds(1);
                continue;
            }
            let delay = (candidate - now).to_std().unwrap_or(Duration::from_secs(1));
            return Ok(delay);
        }
        Err(rust_agent_core::AgentError::WorkflowError(format!(
            "在 1 年内未找到 cron 表达式 '{}' 的下一次触发时间", self.expression
        )))
    }

    fn field_matches(field: &CronField, value: u32) -> bool {
        match field {
            CronField::Any => true,
            CronField::Specific(vals) => vals.contains(&value),
            CronField::Interval(n) => value % n == 0,
        }
    }
}

#[async_trait]
impl IExecutor for CronTrigger {
    fn id(&self) -> &str { &self.node_id }

    fn accepted_types(&self) -> Vec<TypeTag> {
        vec![
            TypeTag::new("initial"),
            TypeTag::new("timer"),
            TypeTag::new(std::any::type_name::<String>()),
        ]
    }

    async fn on_timer(&self, timer_name: &str, _ctx: &dyn IWorkflowContext) -> Result<()> {
        if timer_name == self.timer_name {
            *self.is_fired.lock() = true;
        }
        Ok(())
    }

    async fn handle(
        &self,
        message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
        progress: UnboundedSender<NodeProgress>,
    ) -> Result<HandlerResult> {
        let fired = *self.is_fired.lock();

        if fired {
            *self.is_fired.lock() = false;
            let iter_val = {
                let mut iter = self.iteration.lock();
                *iter += 1;
                *iter
            };

            let _ = progress.send(NodeProgress::Custom {
                key: "cron_fired".into(),
                value: serde_json::json!({"node_id": self.node_id, "expression": self.expression, "iteration": iter_val}),
            });

            if self.max_iterations > 0 && iter_val >= self.max_iterations {
                return Ok(HandlerResult::None);
            }

            match self.next_delay() {
                Ok(delay) => {
                    ctx.schedule_timer(&self.timer_name, delay).await?;
                }
                Err(e) => return Err(e),
            }

            Ok(HandlerResult::Messages(vec![message]))
        } else {
            let delay = self.next_delay()?;
            ctx.schedule_timer(&self.timer_name, delay).await?;
            Ok(HandlerResult::None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_any() {
        assert_eq!(CronTrigger::parse_field("*").unwrap(), CronField::Any);
    }

    #[test]
    fn test_parse_interval() {
        assert_eq!(CronTrigger::parse_field("*/5").unwrap(), CronField::Interval(5));
    }

    #[test]
    fn test_parse_specific() {
        assert_eq!(CronTrigger::parse_field("1,15,30").unwrap(), CronField::Specific(vec![1, 15, 30]));
    }

    #[test]
    fn test_parse_invalid_interval() {
        assert!(CronTrigger::parse_field("*/0").is_err());
    }

    #[test]
    fn test_field_matches() {
        assert!(CronTrigger::field_matches(&CronField::Any, 42));
        let f = CronField::Specific(vec![0, 30]);
        assert!(CronTrigger::field_matches(&f, 0));
        assert!(!CronTrigger::field_matches(&f, 15));
        let f = CronField::Interval(5);
        assert!(CronTrigger::field_matches(&f, 0));
        assert!(!CronTrigger::field_matches(&f, 3));
    }
}

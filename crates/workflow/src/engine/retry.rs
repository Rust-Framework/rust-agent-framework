use std::time::Duration;

/// 重试退避策略
#[derive(Debug, Clone)]
pub enum RetryBackoff {
    /// 无等待
    None,
    /// 固定间隔
    Fixed(Duration),
    /// 指数退避: base_delay * 2^attempt
    Exponential { base: Duration, max: Duration },
}

/// 重试条件
#[derive(Debug, Clone)]
pub enum RetryCondition {
    /// 所有错误都重试
    AllErrors,
    /// 仅包含特定字符串的错误重试
    Contains(String),
}

/// 重试耗尽后的动作
#[derive(Debug, Clone)]
pub enum ExhaustedAction {
    /// 失败：传播错误
    Fail,
    /// 跳过：忽略错误继续
    Skip,
    /// 回退到指定节点
    FallbackNode(String),
}

/// 节点重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 最大重试次数（默认 0 = 不重试）
    pub max_retries: u32,
    /// 退避策略
    pub backoff: RetryBackoff,
    /// 哪些错误触发重试
    pub retry_on: RetryCondition,
    /// 重试耗尽后的处理
    pub on_exhausted: ExhaustedAction,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            backoff: RetryBackoff::Exponential {
                base: Duration::from_secs(1),
                max: Duration::from_secs(60),
            },
            retry_on: RetryCondition::AllErrors,
            on_exhausted: ExhaustedAction::Fail,
        }
    }
}

impl RetryBackoff {
    pub fn delay(&self, attempt: u32) -> Duration {
        match self {
            RetryBackoff::None => Duration::ZERO,
            RetryBackoff::Fixed(d) => *d,
            RetryBackoff::Exponential { base, max } => {
                let d = base.as_millis() as u64 * 2u64.saturating_pow(attempt);
                let capped = d.min(max.as_millis() as u64);
                Duration::from_millis(capped)
            }
        }
    }
}

impl RetryCondition {
    pub fn should_retry(&self, error: &str) -> bool {
        match self {
            RetryCondition::AllErrors => true,
            RetryCondition::Contains(substr) => error.contains(substr),
        }
    }
}

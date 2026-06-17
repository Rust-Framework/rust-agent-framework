use std::sync::Arc;
use std::time::Duration;

use crate::engine::retry::RetryConfig;
use crate::executor::IExecutor;

/// 图中的一个节点
#[derive(Clone)]
pub struct Node {
    pub id: String,
    pub executor: Arc<dyn IExecutor>,
    pub is_output: bool,
    /// 节点重试配置
    pub retry: Option<RetryConfig>,
    /// 单节点超时（覆盖全局配置）
    pub timeout: Option<Duration>,
}

impl Node {
    pub fn new(id: impl Into<String>, executor: Arc<dyn IExecutor>) -> Self {
        Self {
            id: id.into(),
            executor,
            is_output: false,
            retry: None,
            timeout: None,
        }
    }

    pub fn with_output(mut self, is_output: bool) -> Self {
        self.is_output = is_output;
        self
    }

    pub fn with_retry(mut self, config: RetryConfig) -> Self {
        self.retry = Some(config);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

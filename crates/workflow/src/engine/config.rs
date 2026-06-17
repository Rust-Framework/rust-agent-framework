use std::time::Duration;

/// 工作流执行配置
#[derive(Debug, Clone)]
pub struct WorkflowConfig {
    /// 整体超时，过期后强制终止
    pub global_timeout: Option<Duration>,
    /// 单节点默认超时
    pub default_node_timeout: Option<Duration>,
    /// SuperStep 最大并行节点数（0 = 不限制）
    pub max_parallel_nodes: usize,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            global_timeout: None,
            default_node_timeout: None,
            max_parallel_nodes: 0,
        }
    }
}

impl WorkflowConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_global_timeout(mut self, timeout: Duration) -> Self {
        self.global_timeout = Some(timeout);
        self
    }

    pub fn with_node_timeout(mut self, timeout: Duration) -> Self {
        self.default_node_timeout = Some(timeout);
        self
    }

    pub fn with_max_parallel(mut self, max: usize) -> Self {
        self.max_parallel_nodes = max;
        self
    }
}

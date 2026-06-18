use std::sync::Arc;
use std::time::Duration;

use crate::engine::retry::RetryOptions;
use crate::executor::IExecutor;

/// 循环配置 — 用于标记循环回边允许的节点。
///
/// 引擎在 SuperStep 中检查循环条件，与 `ITerminationCondition` 配合使用。
/// 循环状态（迭代计数器、循环变量）可序列化到 checkpoint。
#[derive(Clone)]
pub struct LoopConfig {
    /// 最大迭代次数（0 表示无限制，需由 termination_condition 控制终止）
    pub max_iterations: usize,
    /// 循环变量名 — 引擎自动维护的迭代计数器，存入 state_map
    pub loop_variable: Option<String>,
}

impl LoopConfig {
    pub fn new(max_iterations: usize) -> Self {
        Self {
            max_iterations,
            loop_variable: None,
        }
    }

    pub fn with_variable(mut self, name: impl Into<String>) -> Self {
        self.loop_variable = Some(name.into());
        self
    }

    pub fn unlimited() -> Self {
        Self {
            max_iterations: 0,
            loop_variable: None,
        }
    }
}

/// 图中的一个节点
#[derive(Clone)]
pub struct Node {
    pub id: String,
    pub executor: Arc<dyn IExecutor>,
    pub is_output: bool,
    /// 节点重试配置
    pub retry: Option<RetryOptions>,
    /// 单节点超时（覆盖全局配置）
    pub timeout: Option<Duration>,
    /// 循环配置 — 标记此节点为循环入口
    pub loop_config: Option<LoopConfig>,
}

impl Node {
    pub fn new(id: impl Into<String>, executor: Arc<dyn IExecutor>) -> Self {
        Self {
            id: id.into(),
            executor,
            is_output: false,
            retry: None,
            timeout: None,
            loop_config: None,
        }
    }

    pub fn with_output(mut self, is_output: bool) -> Self {
        self.is_output = is_output;
        self
    }

    pub fn with_retry(mut self, config: RetryOptions) -> Self {
        self.retry = Some(config);
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_loop(mut self, config: LoopConfig) -> Self {
        self.loop_config = Some(config);
        self
    }
}

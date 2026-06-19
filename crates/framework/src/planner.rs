//! Planner 抽象——规划策略可替换
//!
//! 对标 MAF 的 `FunctionCallingStepwisePlanner` / `SequentialPlanner`。
//! MAF 将规划与执行分离，产出 `Plan` DAG（支持依赖、并行、条件分支）。
//!
//! RAF 的 `IPlanner` trait 定义规划策略接口：
//! - `ReActPlanner`：保留 `FunctionInvokingChatClient` 当前行为（LLM 驱动的 ReAct 循环）
//! - `GraphPlanner`（未来）：DAG 并行执行
//!
//! ## 设计原则
//!
//! **原则 3：Trait 即契约，组合即架构。**
//! 将规划策略从工具调用循环中解耦，使 `FunctionInvokingChatClient` 成为执行引擎，
//! 而非规划策略的载体。新策略（如 DAG 并行）可通过实现 `IPlanner` trait 注入，
//! 无需修改 `FunctionInvokingChatClient` 内部状态机。

use async_trait::async_trait;

/// 规划策略接口
///
/// 实现者决定如何根据目标、可用工具和历史消息生成下一步动作。
///
/// ## 与 MAF 的对照
///
/// | MAF | RAF |
/// |-----|-----|
/// | `FunctionCallingStepwisePlanner` | `ReActPlanner`（默认） |
/// | `SequentialPlanner` | `SequentialPlanner`（未来） |
/// | `Plan` DAG | `PlannerDecision`（当前为线性，未来可扩展为 DAG） |
#[async_trait]
pub trait IPlanner: Send + Sync {
    /// 规划器名称（用于日志和诊断）
    fn name(&self) -> &str;

    /// 最大轮次上限
    ///
    /// 对标 MAF 的 `MaximumIterations`。防止无限循环。
    /// `FunctionInvokingChatClient` 会在达到此上限时终止循环。
    fn max_rounds(&self) -> usize;
}

/// ReAct 规划器——保留 `FunctionInvokingChatClient` 当前行为
///
/// LLM 驱动的 ReAct（Reasoning + Acting）循环：
/// 1. LLM 决定调用哪些工具
/// 2. 执行工具，将结果反馈给 LLM
/// 3. 重复直到 LLM 不再请求工具调用
///
/// 这是 RAF 的默认规划策略，行为与 MAF 的 `FunctionCallingStepwisePlanner` 等价。
pub struct ReActPlanner {
    max_rounds: usize,
}

impl ReActPlanner {
    /// 创建 ReAct 规划器
    ///
    /// `max_rounds` 默认为 10，与 `FunctionInvokingChatClient` 历史默认值一致。
    pub fn new() -> Self {
        Self { max_rounds: 10 }
    }

    /// 设置最大轮次
    pub fn with_max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds;
        self
    }
}

impl Default for ReActPlanner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IPlanner for ReActPlanner {
    fn name(&self) -> &str {
        "ReActPlanner"
    }

    fn max_rounds(&self) -> usize {
        self.max_rounds
    }
}

/// 规划决策——规划器产出的下一步动作
///
/// 当前为线性决策（终止/超限），未来可扩展为 DAG 节点。
/// `Continue` 变体已移除——当前 `FunctionInvokingChatClient` 的
/// 工具调用循环由 LLM 响应直接驱动，无需规划器介入。
#[derive(Debug, Clone)]
pub enum PlannerDecision {
    /// 终止循环，返回最终响应
    Stop,
    /// 达到最大轮次
    MaxRoundsExceeded,
}

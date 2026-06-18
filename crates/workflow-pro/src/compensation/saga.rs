use std::sync::Arc;

use rust_agent_core::{AgentError, Result};
use rust_agent_workflow::executor::IExecutor;
use rust_agent_workflow::engine::IWorkflowContext;

/// Saga 恢复策略。
#[derive(Debug, Clone, PartialEq)]
pub enum SagaPolicy {
    /// 向后恢复：失败时逆序执行已完成的步骤的 compensate()
    BackwardRecovery,
    /// 向前恢复：忽略失败，继续执行后续步骤
    ForwardRecovery,
}

/// Saga 步骤定义 —— 每一步包含正向操作和补偿操作。
pub struct SagaStep {
    pub name: String,
    pub action: Arc<dyn IExecutor>,
    pub compensation: Option<Arc<dyn IExecutor>>,
}

impl SagaStep {
    pub fn new(
        name: impl Into<String>,
        action: Arc<dyn IExecutor>,
    ) -> Self {
        Self {
            name: name.into(),
            action,
            compensation: None,
        }
    }

    pub fn with_compensation(mut self, compensation: Arc<dyn IExecutor>) -> Self {
        self.compensation = Some(compensation);
        self
    }
}

/// Saga 编排器 —— 声明式 SAGA 事务编排。
///
/// 基于引擎已有的 `ICompensable` + 逆序 `compensate()` 机制，
/// 提供更高层的声明式 SAGA 编排：
///
/// ```ignore
/// let saga = SagaOrchestrator::new()
///     .step(SagaStep::new("create_order", create_exec).with_compensation(cancel_exec))
///     .step(SagaStep::new("reserve_inventory", reserve_exec).with_compensation(release_exec))
///     .step(SagaStep::new("process_payment", payment_exec).with_compensation(refund_exec))
///     .with_policy(SagaPolicy::BackwardRecovery);
///
/// saga.execute(initial_message, ctx).await?;
/// ```
pub struct SagaOrchestrator {
    steps: Vec<SagaStep>,
    policy: SagaPolicy,
}

impl SagaOrchestrator {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            policy: SagaPolicy::BackwardRecovery,
        }
    }

    pub fn step(mut self, step: SagaStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn with_policy(mut self, policy: SagaPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn steps(&self) -> &[SagaStep] {
        &self.steps
    }

    /// 执行 Saga 的全部步骤。失败时根据策略执行补偿。
    pub async fn execute(
        &self,
        initial_message: Arc<dyn std::any::Any + Send + Sync>,
        ctx: Arc<dyn IWorkflowContext>,
    ) -> Result<Vec<Arc<dyn std::any::Any + Send + Sync>>> {
        let mut results: Vec<Arc<dyn std::any::Any + Send + Sync>> = Vec::new();
        let mut completed_steps: Vec<usize> = Vec::new();

        for (i, step) in self.steps.iter().enumerate() {
            let (progress_tx, mut _progress_rx) =
                tokio::sync::mpsc::unbounded_channel();

            let msg_to_send = if i == 0 {
                initial_message.clone()
            } else {
                results.last().cloned().unwrap_or(initial_message.clone())
            };

            match step.action.handle(msg_to_send, ctx.clone(), progress_tx).await {
                Ok(result) => {
                    match result {
                        rust_agent_workflow::executor::HandlerResult::Messages(msgs) => {
                            results.extend(msgs);
                        }
                        rust_agent_workflow::executor::HandlerResult::Output(out) => {
                            results.push(out);
                        }
                        rust_agent_workflow::executor::HandlerResult::None => {}
                    }
                    completed_steps.push(i);
                }
                Err(e) => {
                    return match self.policy {
                        SagaPolicy::BackwardRecovery => {
                            self.compensate(&completed_steps[..], ctx).await?;
                            Err(AgentError::WorkflowError(format!(
                                "Saga 步骤 '{}' 失败 (已触发 BackwardRecovery): {}",
                                step.name, e
                            )))
                        }
                        SagaPolicy::ForwardRecovery => {
                            tracing::warn!(
                                step = %step.name,
                                error = %e,
                                "Saga 步骤失败，ForwardRecovery 策略继续执行"
                            );
                            continue;
                        }
                    };
                }
            }
        }

        Ok(results)
    }

    /// 逆序执行补偿。
    async fn compensate(
        &self,
        completed_indices: &[usize],
        ctx: Arc<dyn IWorkflowContext>,
    ) -> Result<()> {
        for &i in completed_indices.iter().rev() {
            let step = &self.steps[i];
            if let Some(ref compensation) = step.compensation {
                tracing::info!(step = %step.name, "执行补偿");
                let (progress_tx, mut _rx) = tokio::sync::mpsc::unbounded_channel();
                let empty_msg: Arc<dyn std::any::Any + Send + Sync> =
                    Arc::new("compensation_triggered".to_string());
                if let Err(e) = compensation.handle(empty_msg, ctx.clone(), progress_tx).await {
                    tracing::error!(step = %step.name, error = %e, "补偿执行失败");
                }
            }
        }
        Ok(())
    }
}

impl Default for SagaOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_saga_builder() {
        let saga = SagaOrchestrator::new()
            .with_policy(SagaPolicy::BackwardRecovery);
        assert_eq!(saga.policy, SagaPolicy::BackwardRecovery);
        assert!(saga.steps().is_empty());
    }
}

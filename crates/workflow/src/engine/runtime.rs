use std::sync::Arc;

use rust_agent_core::{BoxStream, ISession, Result};
use tokio::sync::{mpsc, Mutex};

use crate::graph::WorkflowGraph;

use super::engine::{WorkflowEngine, WorkflowOutput};
use super::event::WorkflowEvent;

/// 恢复命令 — 外部通过 WorkflowRuntime 注入
#[derive(Debug)]
pub enum ResumeCommand {
    /// 向指定节点注入消息并恢复执行
    InjectMessage {
        target_node_id: String,
        message: Arc<dyn std::any::Any + Send + Sync>,
    },
    /// 继续执行（不注入新消息）
    Continue,
    /// 中止执行
    Abort,
}

/// 有状态的工作流执行句柄 — 支持暂停后 resume
pub struct WorkflowRuntime {
    resume_tx: mpsc::UnboundedSender<ResumeCommand>,
    done: Mutex<Option<tokio::sync::oneshot::Receiver<Result<()>>>>,
    event_stream: Mutex<Option<BoxStream<'static, WorkflowEvent>>>,
    output_stream: Mutex<Option<BoxStream<'static, Result<WorkflowOutput>>>>,
}

impl WorkflowRuntime {
    /// 启动工作流，返回可交互的 runtime 句柄
    pub async fn start(
        graph: WorkflowGraph,
        initial: Arc<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
    ) -> Result<Self> {
        Self::start_with_engine(WorkflowEngine::new(graph), initial, session).await
    }

    /// 使用自定义引擎配置启动
    pub async fn start_with_engine(
        engine: WorkflowEngine,
        initial: Arc<dyn std::any::Any + Send + Sync>,
        session: Option<Arc<dyn ISession>>,
    ) -> Result<Self> {
        let (resume_tx, resume_rx) = mpsc::unbounded_channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        let (event_stream, output_stream) = engine
            .spawn_run(initial, session, Some(resume_rx), Some(done_tx))
            .await?;

        Ok(Self {
            resume_tx,
            done: Mutex::new(Some(done_rx)),
            event_stream: Mutex::new(Some(event_stream)),
            output_stream: Mutex::new(Some(output_stream)),
        })
    }

    /// 获取事件流（可观测性）
    pub async fn events(&self) -> Option<BoxStream<'static, WorkflowEvent>> {
        self.event_stream.lock().await.take()
    }

    /// 获取输出流
    pub async fn outputs(&self) -> Option<BoxStream<'static, Result<WorkflowOutput>>> {
        self.output_stream.lock().await.take()
    }

    /// 向暂停的工作流注入外部消息（如审批结果）
    pub fn resume(&self, cmd: ResumeCommand) -> Result<()> {
        self.resume_tx
            .send(cmd)
            .map_err(|e| rust_agent_core::AgentError::WorkflowError(format!("resume 失败: {}", e)))
    }

    /// 等待工作流完成（阻塞式）
    pub async fn wait(self) -> Result<()> {
        if let Some(rx) = self.done.lock().await.take() {
            rx.await
                .map_err(|_| {
                    rust_agent_core::AgentError::WorkflowError("工作流任务异常终止".into())
                })??;
        }
        Ok(())
    }
}

/// 便捷函数：启动 runtime 并返回句柄
pub async fn run_resumable(
    graph: WorkflowGraph,
    initial: Arc<dyn std::any::Any + Send + Sync>,
    session: Option<Arc<dyn ISession>>,
) -> Result<WorkflowRuntime> {
    WorkflowRuntime::start(graph, initial, session).await
}

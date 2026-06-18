use std::sync::Arc;

use futures_util::StreamExt;
use rust_agent_core::{
    AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage, IAgent, ISession,
    Result, Content,
};

/// 顺序工作流 — Agent 按顺序执行，每个 Agent 接收前一个 Agent 的输出。
///
/// # MAF 对照
///
/// 对应 MAF 的 `SequentialWorkflow`。Agent 链式执行：
/// 输入 → agent1 → agent2 → ... → 最后一个 Agent 的流式输出。
///
/// # Usage
///
/// ```ignore
/// let workflow = SequentialWorkflow::new()
///     .add_agent(researcher)
///     .add_agent(summarizer);
///
/// let stream = workflow.run(input, session, options).await?;
/// ```
pub struct SequentialWorkflow {
    agents: Vec<Arc<dyn IAgent>>,
}

// Manual Clone since IAgent is not Clone
impl Clone for SequentialWorkflow {
    fn clone(&self) -> Self {
        Self { agents: self.agents.clone() }
    }
}

impl SequentialWorkflow {
    /// 创建空的顺序工作流构建器。
    pub fn new() -> Self {
        Self { agents: Vec::new() }
    }

    /// 从 Agent 列表直接构造（对齐 MAF `SequentialBuilder(participants=[...])`）。
    pub fn from_agents(agents: Vec<Arc<dyn IAgent>>) -> Self {
        Self { agents }
    }

    /// 添加一个 Agent 到序列末尾。
    pub fn add_agent(mut self, agent: Arc<dyn IAgent>) -> Self {
        self.agents.push(agent);
        self
    }

    /// 将工作流包装为 `IAgent`（MAF 设计哲学）。
    ///
    /// `WorkflowBuilder.build() → Workflow.as_agent() → IAgent`。
    /// 返回的 IAgent 支持 `get_subagent(id)` 发现子代理，
    /// 可通过流式事件追踪子代理运行状态。
    pub fn as_agent(self) -> Arc<dyn IAgent> {
        let agents = self.agents.clone();
        let name = if agents.is_empty() {
            "sequential_workflow".to_string()
        } else {
            format!("seq_{}", agents.iter().map(|a| a.id().to_string()).collect::<Vec<_>>().join("_"))
        };

        Arc::new(super::WorkflowAsAgent::new(name, agents, {
            move |input, session, options| {
                let value = self.clone();
                Box::pin(async move { value.run(input, session, options).await })
            }
        }))
    }

    // ── Run ──

    /// 按顺序执行所有 Agent。
    ///
    /// 前 N-1 个 Agent 的输出被收集为文本，作为下一个 Agent 的输入。
    /// 最后一个 Agent 的输出直接流式返回。
    pub async fn run(
        &self,
        mut input: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        if self.agents.is_empty() {
            return Err(rust_agent_core::AgentError::WorkflowError(
                "SequentialWorkflow requires at least one agent".to_string(),
            ));
        }

        let last_idx = self.agents.len() - 1;

        for (i, agent) in self.agents.iter().enumerate() {
            let stream = agent.run(input, session.clone(), options.clone()).await?;

            if i == last_idx {
                return Ok(stream);
            }

            // Collect intermediate response
            let text = collect_stream_text(stream).await;
            input = vec![ChatMessage::assistant(text)];
        }

        // Unreachable (handled by early return above)
        Err(rust_agent_core::AgentError::WorkflowError(
            "Internal error in SequentialWorkflow".to_string(),
        ))
    }
}

async fn collect_stream_text(
    mut stream: BoxStream<'static, Result<AgentResponseResult>>,
) -> String {
    let mut text = String::new();
    while let Some(chunk) = stream.next().await {
        if let Ok(result) = chunk {
            for content in &result.contents {
                match content {
                    Content::Text(ref t) => text.push_str(&t.delta),
                    Content::Reasoning(ref r) => text.push_str(&r.delta),
                    Content::ToolCalled(ref tcr) => {
                        if let Some(ref result) = tcr.result {
                            text.push_str(&format!("\n[工具结果: {}]\n", result));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    text
}

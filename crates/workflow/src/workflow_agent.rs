use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use rust_agent_core::{
    AgentId, AgentMetadata, AgentResponseResult, AgentRunOptions, BoxStream, ChatMessage,
    Content, Event, FinishReason, IAgent, ISession, Result,
};

use crate::builder::WorkflowBuilder;
use crate::engine::event::{NodeChunk, WorkflowEvent};
use crate::graph::WorkflowGraph;

/// WorkflowAgent — 将 WorkflowEngine 包装为 IAgent
///
/// 对应 MAF 的 `Workflow.as_agent()` 模式：
/// 工作流作为一个整体暴露为 Agent。调用 `workflow_agent.run()` 即可启动
/// 整个工作流图执行，获得流式 + 可观测的双通道输出。
///
/// # 流式全链路
///
/// 每个 NodeStreaming 事件被实时转换为 AgentResponseResult 帧，
/// NodeInvoking/NodeCompleted 作为 Event 嵌入，
/// 前端可实时感知每个子代理的执行状态（打字机输出、工具调用、完成/失败）。
pub struct WorkflowAgent {
    id: AgentId,
    metadata: AgentMetadata,
    graph: Arc<WorkflowGraph>,
    /// 子 agent 列表（从 graph 节点中提取的 IAgent 引用）
    sub_agents: Vec<Arc<dyn IAgent>>,
}

impl WorkflowAgent {
    /// 从 WorkflowGraph 创建 Agent
    pub fn new(graph: WorkflowGraph) -> Self {
        // 提取子 agent
        let sub_agents = Self::extract_agents(&graph);

        let id = AgentId::new(format!("workflow_{}", graph.start_node_id()));
        let metadata = AgentMetadata {
            agent_type: "WorkflowAgent".to_string(),
            key: format!("workflow_{}", graph.start_node_id()),
            description: format!(
                "图工作流: {} 节点, {} 条边",
                graph.nodes().len(),
                graph.edges().len()
            ),
        };

        Self {
            id,
            metadata,
            graph: Arc::new(graph),
            sub_agents,
        }
    }

    /// 从 WorkflowBuilder 一步创建
    pub fn from_builder<F>(build: F) -> Result<Self>
    where
        F: FnOnce(WorkflowBuilder) -> Result<WorkflowGraph>,
    {
        let graph = build(WorkflowBuilder::new())?;
        Ok(Self::new(graph))
    }

    /// 从图中提取已注册的 IAgent（通过内省 agent executor nodes）
    fn extract_agents(graph: &WorkflowGraph) -> Vec<Arc<dyn IAgent>> {
        // 简化：遍历 nodes()，检查 executor ID，
        // 跳过 engine 创建的内部节点（如 engine 相关节点）
        // 当前版本的 AgentExecutor 通过构造时传入的 IAgent 来获取
        // 由于类型擦除，这里使用 agent ID 去重
        let mut seen = HashMap::new();
        for node in graph.nodes().values() {
            let executor_id = node.executor.id();
            // agent executor 以用户指定的名称注册
            // 如果节点不是 engine 内部节点，标记为候选
            seen.entry(executor_id.to_string()).or_insert(node.clone());
        }

        // TODO: 当 IExecutor 支持暴露内部 IAgent 时完善此方法
        // 当前版本返回空，待 AgentExecutor 暴露 agent 访问接口后补全
        Vec::new()
    }
}

#[async_trait]
impl IAgent for WorkflowAgent {
    fn id(&self) -> &AgentId {
        &self.id
    }

    fn metadata(&self) -> &AgentMetadata {
        &self.metadata
    }

    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        session: Option<Arc<dyn ISession>>,
        _options: Option<AgentRunOptions>,
    ) -> Result<BoxStream<'static, Result<AgentResponseResult>>> {
        let graph = self.graph.clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<AgentResponseResult>>(64);

        tokio::spawn(async move {
            let initial_message: Box<dyn std::any::Any + Send + Sync> =
                Box::new(messages);

            let engine = crate::engine::WorkflowEngine::new((*graph).clone());

            match engine.run(initial_message, session).await {
                Ok((mut event_stream, mut output_stream)) => {
                    while let Some(event) = event_stream.next().await {
                        match event {
                            WorkflowEvent::NodeInvoking { node_id, .. } => {
                                let meta = make_meta(&node_id);
                                let _ = tx
                                    .send(Ok(AgentResponseResult {
                                        id: Some(node_id.clone()),
                                        model: None,
                                        finish_reason: None,
                                        contents: vec![],
                                        events: vec![Event::ExecutorInvoking(
                                            rust_agent_core::ExecutorInvokingEvent {
                                                meta,
                                                executor_id: node_id.clone(),
                                                executor_type: "Agent".into(),
                                                input_message_count: 1,
                                            },
                                        )],
                                    }))
                                    .await;
                            }
                            WorkflowEvent::NodeStreaming { node_id, chunk } => {
                                let content = match chunk {
                                    NodeChunk::TextDelta { delta } => {
                                        Content::Text(rust_agent_core::TextContent {
                                            delta,
                                            meta: make_meta(&node_id),
                                        })
                                    }
                                    NodeChunk::ReasoningDelta { delta } => {
                                        Content::Reasoning(
                                            rust_agent_core::ReasoningContent {
                                                delta,
                                                meta: make_meta(&node_id),
                                            },
                                        )
                                    }
                                    NodeChunk::ToolCallStart { call_id, name } => {
                                        Content::ToolCallStart(
                                            rust_agent_core::ToolCallStartContent {
                                                call_id,
                                                name,
                                                meta: make_meta(&node_id),
                                            },
                                        )
                                    }
                                    NodeChunk::ToolCallArgs {
                                        call_id,
                                        args_delta,
                                    } => Content::ToolCallArgs(
                                        rust_agent_core::ToolCallArgsContent {
                                            call_id,
                                            args_delta,
                                            meta: make_meta(&node_id),
                                        },
                                    ),
                                    NodeChunk::ToolCallEnd { call_id } => {
                                        Content::ToolCallEnd(
                                            rust_agent_core::ToolCallEndContent {
                                                call_id,
                                                meta: make_meta(&node_id),
                                            },
                                        )
                                    }
                                    NodeChunk::ToolResult { call_id, result } => {
                                        Content::ToolCalled(
                                            rust_agent_core::ToolCalledContent {
                                                call_id,
                                                result: Some(result),
                                            error: None,
                                                meta: make_meta(&node_id),
                                            },
                                        )
                                    }
                                    NodeChunk::UsageUpdate {
                                        prompt_tokens,
                                        completion_tokens,
                                    } => Content::Usage(rust_agent_core::UsageContent {
                                        usage: rust_agent_core::Usage {
                                            prompt_tokens,
                                            completion_tokens,
                                            total_tokens: prompt_tokens + completion_tokens,
                                            prompt_cache_hit_tokens: None,
                                            prompt_cache_miss_tokens: None,
                                            reasoning_tokens: None,
                                        },
                                        meta: make_meta(&node_id),
                                    }),
                                    _ => continue,
                                };
                                let _ = tx
                                    .send(Ok(AgentResponseResult {
                                        id: Some(node_id),
                                        model: None,
                                        finish_reason: None,
                                        contents: vec![content],
                                        events: vec![],
                                    }))
                                    .await;
                            }
                            WorkflowEvent::NodeCompleted { node_id, .. } => {
                                let meta = make_meta(&node_id);
                                let _ = tx
                                    .send(Ok(AgentResponseResult {
                                        id: Some(node_id.clone()),
                                        model: None,
                                        finish_reason: Some(FinishReason::Stop),
                                        contents: vec![],
                                        events: vec![Event::ExecutorInvoked(
                                            rust_agent_core::ExecutorInvokedEvent {
                                                meta,
                                                executor_id: node_id.clone(),
                                                duration_ms: 0,
                                                output_content_count: 0,
                                            },
                                        )],
                                    }))
                                    .await;
                            }
                            WorkflowEvent::NodeFailed { node_id, error } => {
                                let _ = tx
                                    .send(Err(rust_agent_core::AgentError::WorkflowError(
                                        format!("节点 {} 失败: {}", node_id, error),
                                    )))
                                    .await;
                            }
                            WorkflowEvent::WorkflowError { error, .. } => {
                                let _ = tx
                                    .send(Err(rust_agent_core::AgentError::WorkflowError(
                                        error,
                                    )))
                                    .await;
                            }
                            _ => {}
                        }
                    }

                    // 消费输出流
                    while let Some(output_result) = output_stream.next().await {
                        match output_result {
                            Ok(output) => {
                                if let Some(chat_msg) =
                                    output.content.downcast_ref::<ChatMessage>()
                                {
                                    let _ = tx
                                        .send(Ok(AgentResponseResult {
                                            id: Some(output.source_node_id),
                                            model: None,
                                            finish_reason: Some(FinishReason::Stop),
                                            contents: vec![Content::Text(
                                                rust_agent_core::TextContent {
                                                    delta: chat_msg.content.clone(),
                                                    meta: make_meta(""),
                                                },
                                            )],
                                            events: vec![],
                                        }))
                                        .await;
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(e)).await;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                }
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn get_subagent(&self, id: &AgentId) -> Option<Arc<dyn IAgent>> {
        self.sub_agents
            .iter()
            .find(|a| a.id() == id)
            .cloned()
    }

    async fn reset(&self) -> Result<()> {
        for agent in &self.sub_agents {
            agent.reset().await?;
        }
        Ok(())
    }
}

/// 构造 ResponseMetadata 辅助函数
fn make_meta(agent_id: &str) -> rust_agent_core::ResponseMetadata {
    rust_agent_core::ResponseMetadata {
        agent_id: if agent_id.is_empty() {
            None
        } else {
            Some(AgentId::new(agent_id))
        },
        model_id: None,
        executor_id: None,
        timestamp: chrono::Utc::now(),
        properties: std::collections::HashMap::new(),
    }
}

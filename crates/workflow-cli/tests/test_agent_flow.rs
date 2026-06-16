//! 子代理切换与流式输出集成测试
//!
//! 验证 MAF 设计哲学核心闭环：
//!   WorkflowBuilder.build() → Workflow.as_agent() → IAgent
//!   → get_subagent(id) → sub_agent.run() → 独立流式输出
//!
//! 使用 Mock ChatClient + Mock Tool，不依赖真实 API Key。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_util::StreamExt;
use rust_agent_core::{
    AgentId, AgentResponseUpdate, AgentSession,
    BoxStream, ChatClientRunOptions, ChatMessage, Content, FinishReason, IAgent,
    IChatClient, ITool, ModelMetadata, Result,
};
use rust_agent_framework::AgentBuilder;
use rust_agent_workflow::orchestrations::{HandoffWorkflow, WorkflowAsAgent};

// ============================================================
// Mock 基础设施
// ============================================================

/// 模拟 LLM 响应序列的 ChatClient。
struct MockChatClient {
    /// 每次 call 返回的响应，按调用顺序消费
    responses: Vec<Vec<AgentResponseUpdate>>,
    call_count: AtomicUsize,
}

impl MockChatClient {
    fn new(responses: Vec<Vec<AgentResponseUpdate>>) -> Self {
        Self { responses, call_count: AtomicUsize::new(0) }
    }
}

#[async_trait::async_trait]
impl IChatClient for MockChatClient {
    async fn run(
        &self,
        _messages: &[ChatMessage],
        _opts: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(call_index = idx, "MockChatClient::run");

        let response = if idx < self.responses.len() {
            self.responses[idx].clone()
        } else {
            vec![AgentResponseUpdate::Finish {
                finish_reason: FinishReason::Stop,
                usage: None,
            }]
        };
        Ok(Box::pin(futures_util::stream::iter(response.into_iter().map(Ok))))
    }
    fn model_id(&self) -> &str { "mock" }
    fn model_metadata(&self) -> Option<&ModelMetadata> { None }
}

/// 模拟工具，记录调用次数和参数。
#[derive(Clone)]
struct MockTool {
    name: &'static str,
    description: &'static str,
    result: &'static str,
    call_count: Arc<AtomicUsize>,
}

impl MockTool {
    fn new(name: &'static str, result: &'static str) -> Self {
        Self {
            name,
            description: "mock tool",
            result,
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl ITool for MockTool {
    fn name(&self) -> &str { self.name }
    fn description(&self) -> &str { self.description }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }
    async fn execute(&self, _args: serde_json::Value) -> Result<String> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        tracing::debug!(tool = %self.name, count = self.call_count.load(Ordering::Relaxed), "MockTool::execute");
        Ok(self.result.to_string())
    }
}

/// 构造一个简单的 Mock Agent。
fn mock_agent(id: &str, instructions: &str, description: &str) -> Arc<dyn IAgent> {
    let client = MockChatClient::new(vec![vec![
        AgentResponseUpdate::TextDelta { delta: format!("[{}] I am the {} specialist.", id, id) },
        AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
    ]]);
    let builder = AgentBuilder::new(id)
        .chat_client(client)
        .instructions(instructions)
        .with_description(description);
    builder.build().unwrap()
}

// ============================================================
// 测试套件
// ============================================================

/// 测试：WorkflowBuilder → as_agent() → IAgent 统一门面
#[tokio::test]
async fn test_workflow_as_agent_produces_ia_agent_facade() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let coder = mock_agent("coder", "You are a coder.", "代码专家");
    let writer = mock_agent("writer", "You are a writer.", "写作专家");
    let triage = mock_agent("triage", "Route requests.", "路由专家");

    let workflow = HandoffWorkflow::new()
        .triage(triage)
        .agent(coder.clone())
        .agent(writer.clone())
        .build()
        .expect("Should build workflow");

    tracing::info!("Step 1: as_agent() → IAgent facade");
    let agent: Arc<dyn IAgent> = workflow.as_agent();
    tracing::info!(agent_id = %agent.id(), agent_type = %agent.metadata().agent_type, "IAgent facade created");

    assert!(agent.id().to_string().contains("handoff"), "ID should contain 'handoff'");
    assert_eq!(agent.metadata().agent_type, "WorkflowAgent");

    tracing::info!("Step 2: get_subagent() — discover child agents");
    let sub_coder = agent.get_subagent(&coder.id().clone());
    let sub_writer = agent.get_subagent(&writer.id().clone());
    let sub_none = agent.get_subagent(&AgentId::new("nonexistent"));

    tracing::info!(
        coder_found = sub_coder.is_some(),
        writer_found = sub_writer.is_some(),
        nonexistent_found = sub_none.is_some(),
        "get_subagent results"
    );
    assert!(sub_coder.is_some(), "Should find coder sub-agent");
    assert!(sub_writer.is_some(), "Should find writer sub-agent");
    assert!(sub_none.is_none(), "Should not find nonexistent agent");
}

/// 测试：子代理独立 run() 产生流式输出
#[tokio::test]
async fn test_sub_agent_independent_streaming() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    let coder = mock_agent("coder", "You are a coder.", "代码专家");
    let writer = mock_agent("writer", "You are a writer.", "写作专家");
    let triage = mock_agent("triage", "Route requests.", "路由专家");

    let workflow = HandoffWorkflow::new()
        .triage(triage)
        .agent(coder.clone())
        .agent(writer.clone())
        .build()
        .expect("Should build workflow");

    let agent: Arc<dyn IAgent> = workflow.as_agent();
    let sub_coder = agent.get_subagent(&coder.id().clone()).expect("Should find coder");

    tracing::info!(sub_agent_id = %sub_coder.id(), "Starting sub-agent independent run");
    let session = Arc::new(AgentSession::with_id("test-sub"));
    let stream = sub_coder
        .run(
            vec![ChatMessage::user("write Python code")],
            Some(session),
            None,
        )
        .await
        .expect("Sub-agent run should succeed");

    tracing::info!("Consuming sub-agent stream");
    let results: Vec<_> = stream.collect().await;

    let text: String = results
        .iter()
        .filter_map(|r| r.as_ref().ok())
        .flat_map(|r| &r.contents)
        .filter_map(|c| match c {
            Content::Text(t) => Some(t.delta.as_str()),
            _ => None,
        })
        .collect();

    tracing::info!(chars = text.len(), "Sub-agent stream consumed");
    assert!(!text.is_empty(), "Should produce text output");
    assert!(text.contains("coder"), "Output should identify as coder");
}

/// 测试：父代理 triage 路由 + 子代理流式输出的完整链路
#[tokio::test]
async fn test_parent_triage_routing_to_sub_agent_with_streaming() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();

    // Triage: 第一轮返回路由指令，第二轮返回实际响应
    let triage_client = MockChatClient::new(vec![
        // 第一轮：triage 响应（确定路由目标）
        vec![
            AgentResponseUpdate::TextDelta { delta: "coder".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
        // 第二轮：实际执行（如果 triage 又被调用）
        vec![
            AgentResponseUpdate::TextDelta { delta: "[triage final] done".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
    ]);

    let triage = AgentBuilder::new("triage")
        .chat_client(triage_client)
        .instructions("Route to coder or writer. Reply with just the agent name.")
        .with_description("路由专家")
        .build()
        .unwrap();

    // Coder: 工具调用 + 文本输出
    let read_file_tool = MockTool::new("read_file", "mock content");
    let coder_client = MockChatClient::new(vec![
        vec![
            AgentResponseUpdate::ToolCallStart { id: "tc1".to_string(), name: "read_file".to_string() },
            AgentResponseUpdate::ToolCallArgs { id: "tc1".to_string(), args_delta: r#"{"path":"main.py"}"#.to_string() },
            AgentResponseUpdate::ToolCallEnd { id: "tc1".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
        vec![
            AgentResponseUpdate::TextDelta { delta: "def quick_sort(arr): ...".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
    ]);
    let coder = AgentBuilder::new("code-expert")
        .chat_client(coder_client)
        .instructions("You are a coder.")
        .with_tool(read_file_tool.clone())
        .with_description("代码专家")
        .build()
        .unwrap();
    let coder_id = coder.id().clone();

    // Writer: 纯文本
    let writer_client = MockChatClient::new(vec![vec![
        AgentResponseUpdate::TextDelta { delta: "Here is the documentation...".to_string() },
        AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
    ]]);
    let writer = AgentBuilder::new("doc-expert")
        .chat_client(writer_client)
        .instructions("You are a writer.")
        .with_description("文档专家")
        .build()
        .unwrap();
    let writer_id = writer.id().clone();

    let workflow = HandoffWorkflow::new()
        .triage(triage)
        .agent(coder)
        .agent(writer)
        .build()
        .expect("Should build workflow");

    let agent: Arc<dyn IAgent> = workflow.as_agent();

    // ── 关键节点 1: 获取子代理 ──
    tracing::info!(
        agent_id = %agent.id(),
        description = %agent.metadata().description,
        "🔍 get_subagent: looking up child agents"
    );
    let sub_coder = agent.get_subagent(&coder_id);
    let sub_writer = agent.get_subagent(&writer_id);
    tracing::info!(
        coder_found = sub_coder.is_some(),
        writer_found = sub_writer.is_some(),
        "✅ all sub-agents discovered"
    );

    // ── 关键节点 2: 子代理独立流式运行 ──
    let coder_agent = sub_coder.unwrap();
    tracing::info!(
        sub_agent_id = %coder_agent.id(),
        "🚀 running sub-agent independently"
    );
    let sub_session = Arc::new(AgentSession::with_id("sub-test"));
    let sub_stream = coder_agent
        .run(vec![ChatMessage::user("write quicksort")], Some(sub_session), None)
        .await
        .expect("Sub-agent should start");

    let mut sub_chunks = 0usize;
    let mut sub_text = String::new();
    {
        let mut s = Box::pin(sub_stream);
        while let Some(chunk) = s.next().await {
            match chunk {
                Ok(result) => {
                    for c in &result.contents {
                        if let Content::Text(ref t) = c {
                            sub_text.push_str(&t.delta);
                            tracing::trace!(chunk = sub_chunks, text_len = t.delta.len(), "📝 sub-agent text delta");
                        }
                    }
                    sub_chunks += 1;
                }
                Err(e) => tracing::warn!(error = %e, "⚠️ sub-agent error"),
            }
        }
    }
    tracing::info!(
        chunks = sub_chunks,
        chars = sub_text.len(),
        "✅ sub-agent streaming completed"
    );
    assert!(sub_chunks >= 3, "Should have multiple streaming chunks (got {})", sub_chunks);
    assert!(!sub_text.is_empty());

    // ── 关键节点 3: 父代理 triage 路由 ──
    tracing::info!("🔄 parent agent triage routing");
    let parent_session = Arc::new(AgentSession::with_id("parent-test"));
    let parent_stream = agent
        .run(vec![ChatMessage::user("write code")], Some(parent_session), None)
        .await
        .expect("Parent should start");

    let mut parent_chunks = 0usize;
    let mut parent_text = String::new();
    {
        let mut ps = Box::pin(parent_stream);
        while let Some(chunk) = ps.next().await {
            match chunk {
                Ok(result) => {
                    for c in &result.contents {
                        match c {
                            Content::Text(ref t) => {
                                parent_text.push_str(&t.delta);
                                tracing::trace!(chunk = parent_chunks, "📝 parent text delta");
                            }
                            Content::ToolCallStart(inner) => {
                                tracing::info!(tool = %inner.name, "🔧 parent: tool call started");
                            }
                            Content::ToolCalled(inner) => {
                                tracing::info!(
                                    tool = %inner.call_id,
                                    has_result = inner.result.is_some(),
                                    "✅ parent: tool completed"
                                );
                            }
                            _ => {}
                        }
                    }
                    parent_chunks += 1;
                }
                Err(e) => tracing::warn!(error = %e, "⚠️ parent error"),
            }
        }
    }
    tracing::info!(
        chunks = parent_chunks,
        chars = parent_text.len(),
        "✅ parent triage routing completed"
    );
    assert!(parent_chunks > 0, "Parent should have streaming output");
    assert!(!parent_text.is_empty(), "Parent should produce text");

    tracing::info!("🎯 ALL VERIFIED: as_agent → get_subagent → sub stream → parent triage");
}

/// 测试：reset() 递归重置所有子代理
#[tokio::test]
async fn test_workflow_agent_reset_recursively() {
    let coder = mock_agent("coder", "coder instructions", "coder");
    let writer = mock_agent("writer", "writer instructions", "writer");
    let triage = mock_agent("triage", "triage instructions", "triage");

    let workflow = HandoffWorkflow::new()
        .triage(triage.clone())
        .agent(coder.clone())
        .agent(writer.clone())
        .build()
        .unwrap();

    let agent: Arc<dyn IAgent> = workflow.as_agent();

    // reset 应该成功（递归重置所有子代理）
    tracing::info!("🔄 resetting workflow agent and all sub-agents");
    agent.reset().await.expect("Reset should succeed");
    tracing::info!("✅ reset completed successfully");

    // 验证子代理依然可访问
    assert!(agent.get_subagent(&coder.id().clone()).is_some());
    assert!(agent.get_subagent(&writer.id().clone()).is_some());
}

// ============================================================
// 辅助断言宏（编译期类型检查）
// ============================================================

/// 静态断言：WorkflowAsAgent 实现了 IAgent
#[allow(dead_code)]
fn assert_workflow_as_agent_implements_iagent() {
    fn assert_ia<T: IAgent + ?Sized>() {}
    assert_ia::<WorkflowAsAgent>();
}

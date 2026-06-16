//! 流程编排集成测试 — 使用 Mock 客户端验证核心逻辑
//!
//! 不依赖真实 API Key，通过 Mock 验证：
//! 1. FunctionInvokingChatClient 工具调用循环不再卡死（Bug 修复验证）
//! 2. Session TTL cleanup 正确驱逐
//! 3. WorkflowEngine Checkpoint 生命周期

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_util::StreamExt;
use rust_agent_core::{
    AgentResponseUpdate, AgentSession, BoxStream, ChatClientRunOptions, ChatMessage,
    FinishReason, IChatClient, ISession, ISessionStore, ITool, ModelMetadata, Result,
    SessionTTLOptions,
};
use rust_agent_framework::{
    InMemorySessionStore, FunctionInvokingChatClient,
};
use rust_agent_workflow::{
    WorkflowBuilder, CheckpointManager, InMemoryCheckpointStore,
    FunctionExecutor, WorkflowEngine,
};
use tokio::sync::Mutex;

// ============================================================
// Mock 工具：记录调用次数
// ============================================================

struct CallCounter {
    count: Mutex<usize>,
}
impl CallCounter {
    fn new() -> Self { Self { count: Mutex::new(0) } }
    async fn inc(&self) -> usize { let mut c = self.count.lock().await; *c += 1; *c }
    async fn value(&self) -> usize { *self.count.lock().await }
}

#[derive(Clone)]
struct MockReadFileTool {
    counter: Arc<CallCounter>,
}
impl MockReadFileTool {
    fn new() -> Self { Self { counter: Arc::new(CallCounter::new()) } }
}

#[async_trait::async_trait]
impl ITool for MockReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str { "Reads a file" }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}})
    }
    async fn execute(&self, args: serde_json::Value) -> Result<String> {
        let _path = args.get("path").and_then(|v| v.as_str()).unwrap_or("unknown");
        self.counter.inc().await;
        Ok("mock file content".to_string())
    }
}

// ============================================================
// Mock ChatClient：模拟 LLM 返回工具调用 → 文本响应
// ============================================================

struct ToolLoopMockClient {
    responses: Vec<Vec<AgentResponseUpdate>>,
    call_count: AtomicUsize,
}

impl ToolLoopMockClient {
    fn new(responses: Vec<Vec<AgentResponseUpdate>>) -> Self {
        Self { responses, call_count: AtomicUsize::new(0) }
    }
    fn call_count(&self) -> usize { self.call_count.load(Ordering::Relaxed) }
}

#[async_trait::async_trait]
impl IChatClient for ToolLoopMockClient {
    async fn run(&self, _messages: &[ChatMessage], _opts: ChatClientRunOptions) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
        let response = if idx < self.responses.len() {
            self.responses[idx].clone()
        } else {
            vec![
                AgentResponseUpdate::TextDelta { delta: "[default]".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ]
        };
        Ok(Box::pin(futures_util::stream::iter(response.into_iter().map(Ok))))
    }
    fn model_id(&self) -> &str { "mock-model" }
    fn model_metadata(&self) -> Option<&ModelMetadata> { None }
}

// ============================================================
// 测试 1：工具调用循环不卡死（Bug 修复验证）
// ============================================================

#[tokio::test]
async fn test_tool_loop_no_longer_stuck() {
    // 模拟 LLM 第一轮返回工具调用，第二轮返回文本
    let mock = Arc::new(ToolLoopMockClient::new(vec![
        vec![
            AgentResponseUpdate::ToolCallStart { id: "c1".to_string(), name: "read_file".to_string() },
            AgentResponseUpdate::ToolCallArgs { id: "c1".to_string(), args_delta: r#"{"path": "Cargo.toml"}"#.to_string() },
            AgentResponseUpdate::ToolCallEnd { id: "c1".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
        vec![
            AgentResponseUpdate::TextDelta { delta: "Tool executed, summary follows.".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
    ]));

    let tool = MockReadFileTool::new();
    let client = FunctionInvokingChatClient::new(
        mock.clone(),
        vec![Arc::new(tool.clone())],
    ).with_max_rounds(3);

    let stream = client
        .run(&[ChatMessage::user("read Cargo.toml")], ChatClientRunOptions::default())
        .await
        .unwrap();

    let results: Vec<_> = stream.collect().await;

    // 验证：inner 只被调用 2 次（1 tool + 1 text），不会卡在循环中
    assert_eq!(mock.call_count(), 2, "Should call inner exactly twice, not stuck in loop");

    // 验证：工具只被调用 1 次
    assert_eq!(tool.counter.value().await, 1, "Tool should be called exactly once");

    // 验证：最终流输出包含文本
    let has_final_text = results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta.contains("Tool executed")));
    assert!(has_final_text, "Should contain text output from second LLM round");

    // 验证：没有 ToolCalls finish 信号泄漏
    let leak_count = results.iter()
        .filter(|r| matches!(r, Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::ToolCalls, .. })))
        .count();
    assert_eq!(leak_count, 0, "ToolCalls finish signals should be consumed by loop");
}

// ============================================================
// 测试 2：多轮工具调用（3 轮）
// ============================================================

#[tokio::test]
async fn test_multi_round_tool_loop_progresses() {
    let mock = Arc::new(ToolLoopMockClient::new(vec![
        // Round 0: tool call
        vec![
            AgentResponseUpdate::ToolCallStart { id: "c1".to_string(), name: "read_file".to_string() },
            AgentResponseUpdate::ToolCallArgs { id: "c1".to_string(), args_delta: r#"{"path": "a"}"#.to_string() },
            AgentResponseUpdate::ToolCallEnd { id: "c1".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
        // Round 1: tool call again
        vec![
            AgentResponseUpdate::ToolCallStart { id: "c2".to_string(), name: "read_file".to_string() },
            AgentResponseUpdate::ToolCallArgs { id: "c2".to_string(), args_delta: r#"{"path": "b"}"#.to_string() },
            AgentResponseUpdate::ToolCallEnd { id: "c2".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
        // Round 2: final text
        vec![
            AgentResponseUpdate::TextDelta { delta: "Done.".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ],
    ]));

    let tool = MockReadFileTool::new();
    let client = FunctionInvokingChatClient::new(
        mock.clone(),
        vec![Arc::new(tool.clone())],
    ).with_max_rounds(5);

    let stream = client
        .run(&[ChatMessage::user("read files")], ChatClientRunOptions::default())
        .await
        .unwrap();
    let results: Vec<_> = stream.collect().await;

    assert_eq!(mock.call_count(), 3, "3 rounds: 2 tool calls + 1 final text");
    assert_eq!(tool.counter.value().await, 2, "Tool called 2 times");

    let has_final = results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta == "Done."));
    assert!(has_final, "Should reach final text after 2 tool rounds");
}

// ============================================================
// 测试 3：Session TTL cleanup
// ============================================================

#[tokio::test]
async fn test_session_cleanup_eviction() {
    let store = InMemorySessionStore::new()
        .with_ttl(SessionTTLOptions {
            max_idle_secs: Some(1),
            max_lifetime_secs: None,
            cleanup_interval_secs: 60,
        });

    for i in 0..5 {
        let s = Arc::new(AgentSession::with_id(&format!("s-{}", i)));
        store.save_session(s.as_ref()).await.unwrap();
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let removed = store.cleanup_expired().await.unwrap();
    assert_eq!(removed, 5, "All 5 sessions should be expired");
}

// ============================================================
// 测试 4：WorkflowEngine Checkpoint 生命周期
// ============================================================

#[tokio::test]
async fn test_workflow_engine_checkpoint_lifecycle() {
    let store = Arc::new(InMemoryCheckpointStore::new());
    let cp_manager = Arc::new(CheckpointManager::with_default_config(store));

    let graph = WorkflowBuilder::new()
        .add_node("node1", Arc::new(FunctionExecutor::new(
            "node1",
            |msg: String| vec![format!("{} → processed", msg)]
        )))
        .set_start("node1")
        .with_output_from("node1")
        .build()
        .unwrap();

    let engine = WorkflowEngine::new(graph)
        .with_checkpoint_manager(cp_manager);

    let session: Arc<dyn ISession> = Arc::new(AgentSession::with_id("cp-test"));

    let (mut events, _outputs) = engine
        .run(Box::new("test message".to_string()), Some(session))
        .await
        .unwrap();

    let mut event_count = 0;
    let timeout = tokio::time::sleep(std::time::Duration::from_secs(3));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            Some(_) = events.next() => { event_count += 1; }
            _ = &mut timeout => { break; }
        }
    }
    assert!(event_count >= 4, "Should produce WorkflowStarted + SuperStep events + WorkflowCompleted");
}

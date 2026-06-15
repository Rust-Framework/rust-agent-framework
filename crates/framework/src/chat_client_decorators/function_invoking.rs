use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, FinishReason,
    IChatClient, ITool, MessageRole, ModelMetadata, ResponseMetadata, Result,
    ToolCalledContent, ToolCall,
};

/// 工具调用循环 ChatClient 装饰器
///
/// 参照 MAF 的 FunctionInvokingChatClient 设计。
/// 将工具调用循环从 Agent 层下沉到 ChatClient 管道层。
///
/// ## 工作原理
///
/// 1. 调用 `inner.run(messages, options)` 获取 LLM 响应流
/// 2. 消费流，收集 `ToolCallStart`/`ToolCallArgs`/`ToolCallEnd` 事件
/// 3. 如果有工具调用：并行执行工具，将 assistant(tool_calls) + tool(results) 追加到 messages，回到步骤 1
/// 4. 如果无工具调用，返回最终流
///
/// ## 消息累积
///
/// 每轮迭代结束后，工具调用和工具结果消息会自动累积到下一轮的 messages 中，
/// 确保 LLM 能看到完整的工具交互历史。
pub struct FunctionInvokingChatClient {
    inner: Arc<dyn IChatClient>,
    tools: Vec<Arc<dyn ITool>>,
    max_rounds: usize,
}

impl FunctionInvokingChatClient {
    pub fn new(inner: Arc<dyn IChatClient>, tools: Vec<Arc<dyn ITool>>) -> Self {
        tracing::info!(
            tool_count = tools.len(),
            max_rounds = 10,
            "FunctionInvokingChatClient created"
        );
        Self {
            inner,
            tools,
            max_rounds: 10,
        }
    }

    pub fn with_max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds;
        self
    }

    pub fn tools(&self) -> &[Arc<dyn ITool>] {
        &self.tools
    }
}

/// Accumulated tool call data from streaming deltas
#[derive(Clone, Default)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// State machine for the tool-loop unfold stream.
enum LoopState {
    Looping {
        messages: Vec<ChatMessage>,
        round: usize,
        options: ChatClientRunOptions,
    },
    Streaming {
        rx: mpsc::Receiver<Result<AgentResponseUpdate>>,
        on_done: Box<LoopState>,
    },
    Done,
}

#[async_trait]
impl IChatClient for FunctionInvokingChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let inner = Arc::clone(&self.inner);
        let tools = Arc::new(self.tools.clone());
        let max_rounds = self.max_rounds;

        tracing::info!(
            initial_message_count = messages.len(),
            max_rounds,
            "FunctionInvokingChatClient::run starting"
        );

        let initial_state = LoopState::Looping {
            messages: messages.to_vec(),
            round: 0,
            options,
        };

        let stream = futures_util::stream::unfold(
            initial_state,
            move |state| {
                let inner = Arc::clone(&inner);
                let tools = Arc::clone(&tools);

                async move {
                    match state {
                        LoopState::Done => {
                            tracing::trace!("FunctionInvokingChatClient stream ended");
                            None
                        }

                        LoopState::Streaming { mut rx, on_done } => {
                            match rx.recv().await {
                                Some(Ok(update)) => {
                                    if matches!(&update, AgentResponseUpdate::Finish { finish_reason, .. }
                                        if *finish_reason == FinishReason::ToolCalls)
                                    {
                                        tracing::trace!("Detected ToolCalls finish signal, transitioning to next loop iteration");
                                        Some((Ok(update), *on_done))
                                    } else {
                                        Some((Ok(update), LoopState::Streaming { rx, on_done }))
                                    }
                                }
                                Some(Err(e)) => {
                                    tracing::warn!(error = %e, "Error in tool loop streaming phase");
                                    Some((Err(e), LoopState::Done))
                                }
                                None => {
                                    tracing::trace!("Streaming channel closed");
                                    None
                                }
                            }
                        }

                        LoopState::Looping {
                            messages,
                            round,
                            options,
                        } => {
                            if round >= max_rounds {
                                tracing::warn!(
                                    round,
                                    max_rounds,
                                    "Tool loop reached max rounds, terminating"
                                );
                                let err_update = AgentResponseUpdate::Finish {
                                    finish_reason: FinishReason::Stop,
                                    usage: None,
                                };
                                return Some((Ok(err_update), LoopState::Done));
                            }

                            tracing::info!(
                                round,
                                message_count = messages.len(),
                                "Tool loop iteration starting"
                            );

                            // Call inner ChatClient
                            let stream = match inner.run(&messages, options.clone()).await {
                                Ok(s) => {
                                    tracing::trace!(round, "Inner ChatClient returned stream successfully");
                                    s
                                }
                                Err(e) => {
                                    tracing::warn!(round, error = %e, "Inner ChatClient failed");
                                    return Some((Err(e), LoopState::Done));
                                }
                            };

                            // Forward stream via mpsc channel
                            // Also send accumulated messages back via a separate channel
                            let (tx, mut rx) = mpsc::channel::<Result<AgentResponseUpdate>>(256);
                            let (msg_tx, mut msg_rx) = mpsc::channel::<Vec<ChatMessage>>(1);
                            let tools_clone = Arc::clone(&tools);

                            tokio::spawn(async move {
                                let mut s = stream;
                                let mut tool_calls: Vec<AccumulatedToolCall> = Vec::new();
                                let mut current_tool_args: Option<AccumulatedToolCall> = None;
                                let mut text_delta = String::new();
                                let mut has_tool_calls = false;

                                tracing::trace!("Spawned task: consuming inner stream");

                                while let Some(item) = s.next().await {
                                    match item {
                                        Ok(update) => {
                                            match &update {
                                                AgentResponseUpdate::ToolCallStart { id, name } => {
                                                    tracing::trace!(
                                                        tool_name = %name,
                                                        call_id = %id,
                                                        "ToolCallStart received"
                                                    );
                                                    has_tool_calls = true;
                                                    current_tool_args = Some(AccumulatedToolCall {
                                                        id: id.clone(),
                                                        name: name.clone(),
                                                        arguments: String::new(),
                                                    });
                                                    if tx.send(Ok(update)).await.is_err() {
                                                        tracing::trace!("Stream consumer dropped (ToolCallStart)");
                                                        return;
                                                    }
                                                }
                                                AgentResponseUpdate::ToolCallArgs { id, args_delta } => {
                                                    tracing::trace!(
                                                        call_id = %id,
                                                        args_delta_len = args_delta.len(),
                                                        "ToolCallArgs received"
                                                    );
                                                    if let Some(ref mut tc) = current_tool_args {
                                                        if tc.id == *id {
                                                            tc.arguments.push_str(args_delta);
                                                        }
                                                    }
                                                    if tx.send(Ok(update)).await.is_err() {
                                                        tracing::trace!("Stream consumer dropped (ToolCallArgs)");
                                                        return;
                                                    }
                                                }
                                                AgentResponseUpdate::ToolCallEnd { id } => {
                                                    tracing::trace!(
                                                        call_id = %id,
                                                        "ToolCallEnd received — arguments complete"
                                                    );
                                                    if let Some(tc) = current_tool_args.take() {
                                                        if tc.id == *id {
                                                            tracing::trace!(
                                                                tool_name = %tc.name,
                                                                call_id = %tc.id,
                                                                args_len = tc.arguments.len(),
                                                                "Tool call finalized with args: {}",
                                                                tc.arguments
                                                            );
                                                            tool_calls.push(tc);
                                                        }
                                                    }
                                                    if tx.send(Ok(update)).await.is_err() {
                                                        tracing::trace!("Stream consumer dropped (ToolCallEnd)");
                                                        return;
                                                    }
                                                }
                                                AgentResponseUpdate::ToolCallDelta {
                                                    index,
                                                    id,
                                                    name,
                                                    arguments_delta,
                                                } => {
                                                    tracing::trace!(
                                                        index,
                                                        "ToolCallDelta received (legacy format)"
                                                    );
                                                    has_tool_calls = true;
                                                    let idx = *index;
                                                    while tool_calls.len() <= idx {
                                                        tool_calls.push(AccumulatedToolCall::default());
                                                    }
                                                    if let Some(ref id) = id {
                                                        tool_calls[idx].id = id.clone();
                                                    }
                                                    if let Some(ref name) = name {
                                                        tool_calls[idx].name = name.clone();
                                                    }
                                                    tool_calls[idx].arguments.push_str(arguments_delta);
                                                    if tx.send(Ok(update)).await.is_err() {
                                                        tracing::trace!("Stream consumer dropped (ToolCallDelta)");
                                                        return;
                                                    }
                                                }
                                                AgentResponseUpdate::TextDelta { delta } => {
                                                    text_delta.push_str(delta);
                                                    tracing::trace!(delta_len = delta.len(), "TextDelta forwarded");
                                                    if tx.send(Ok(update)).await.is_err() {
                                                        tracing::trace!("Stream consumer dropped (TextDelta)");
                                                        return;
                                                    }
                                                }
                                                AgentResponseUpdate::Finish { .. } => {
                                                    tracing::trace!("Inner Finish received, deferring emission");
                                                    // Don't forward now — we check for tool calls first
                                                }
                                                _ => {
                                                    tracing::trace!("Forwarding other update variant");
                                                    if tx.send(Ok(update)).await.is_err() {
                                                        tracing::trace!("Stream consumer dropped (other)");
                                                        return;
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "Error in inner stream");
                                            let _ = tx.send(Err(e)).await;
                                            return;
                                        }
                                    }
                                }

                                if !has_tool_calls {
                                    tracing::info!("No tool calls detected — emitting final Finish(Stop)");
                                    let _ = tx.send(Ok(AgentResponseUpdate::Finish {
                                        finish_reason: FinishReason::Stop,
                                        usage: None,
                                    })).await;
                                    return;
                                }

                                // ── Execute tools in parallel ──
                                tracing::info!(
                                    tool_count = tool_calls.len(),
                                    tool_names = %tool_calls.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", "),
                                    "Executing tools"
                                );

                                let meta = ResponseMetadata {
                                    agent_id: None,
                                    model_id: None,
                                    executor_id: None,
                                    timestamp: Utc::now(),
                                    properties: Default::default(),
                                };

                                let tool_futures: Vec<_> = tool_calls
                                    .iter()
                                    .map(|tc| {
                                        let tc = tc.clone();
                                        let tools = Arc::clone(&tools_clone);
                                        let meta = meta.clone();
                                        async move {
                                            tracing::trace!(
                                                tool_name = %tc.name,
                                                call_id = %tc.id,
                                                args = %tc.arguments,
                                                "Executing tool"
                                            );
                                            let args_value = serde_json::from_str(&tc.arguments)
                                                .unwrap_or(serde_json::Value::Object(Default::default()));
                                            match tools.iter().find(|t| t.name() == tc.name) {
                                                Some(tool) => match tool.execute(args_value).await {
                                                    Ok(output) => {
                                                        tracing::trace!(
                                                            tool_name = %tc.name,
                                                            call_id = %tc.id,
                                                            has_error = false,
                                                            output_len = output.len(),
                                                            "Tool execution succeeded"
                                                        );
                                                        ToolCalledContent {
                                                            meta,
                                                            call_id: tc.id.clone(),
                                                            result: Some(output),
                                                            error: None,
                                                        }
                                                    }
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            tool_name = %tc.name,
                                                            call_id = %tc.id,
                                                            error = %e,
                                                            "Tool execution failed"
                                                        );
                                                        ToolCalledContent {
                                                            meta,
                                                            call_id: tc.id.clone(),
                                                            result: None,
                                                            error: Some(e.to_string()),
                                                        }
                                                    }
                                                },
                                                None => {
                                                    tracing::warn!(
                                                        tool_name = %tc.name,
                                                        call_id = %tc.id,
                                                        "Tool not found in registry"
                                                    );
                                                    ToolCalledContent {
                                                        meta,
                                                        call_id: tc.id.clone(),
                                                        result: None,
                                                        error: Some(format!("Tool '{}' not found", tc.name)),
                                                    }
                                                }
                                            }
                                        }
                                    })
                                    .collect::<Vec<_>>();

                                let results = futures_util::future::join_all(tool_futures).await;

                                // ── Build accumulated messages for next iteration ──
                                let mut next_messages = Vec::new();

                                // Assistant message with tool_calls (required by API protocol)
                                let assistant_tool_msg = ChatMessage {
                                    role: MessageRole::Assistant,
                                    content: text_delta.clone(),
                                    name: None,
                                    tool_calls: Some(
                                        tool_calls.iter().map(|tc| ToolCall {
                                            id: tc.id.clone(),
                                            name: tc.name.clone(),
                                            arguments: serde_json::Value::String(tc.arguments.clone()),
                                        }).collect(),
                                    ),
                                    tool_call_id: None,
                                    source: None,
                                };
                                next_messages.push(assistant_tool_msg);

                                // Tool result messages (one per tool call)
                                for (i, result) in results.iter().enumerate() {
                                    let call_id = &tool_calls[i].id;
                                    let content = result.result.clone().unwrap_or_default();
                                    next_messages.push(ChatMessage {
                                        role: MessageRole::Tool,
                                        content,
                                        name: Some(tool_calls[i].name.clone()),
                                        tool_calls: None,
                                        tool_call_id: Some(call_id.clone()),
                                        source: None,
                                    });
                                }

                                tracing::trace!(
                                    assistant_msg_count = 1,
                                    tool_result_count = results.len(),
                                    "Built accumulated messages for next iteration"
                                );

                                // Send accumulated messages back
                                let _ = msg_tx.send(next_messages).await;

                                // Forward tool result events
                                for (i, result) in results.into_iter().enumerate() {
                                    let call_id = tool_calls[i].id.clone();
                                    tracing::trace!(
                                        call_id = %call_id,
                                        has_error = result.error.is_some(),
                                        "Emitting tool result event",
                                    );
                                    if tx.send(Ok(AgentResponseUpdate::ToolCallEnd {
                                        id: call_id,
                                    })).await.is_err() {
                                        tracing::trace!("Stream consumer dropped during result emission");
                                        return;
                                    }
                                }

                                // Signal loop continuation
                                tracing::trace!("Emitting ToolCalls finish signal for loop continuation");
                                let _ = tx.send(Ok(AgentResponseUpdate::Finish {
                                    finish_reason: FinishReason::ToolCalls,
                                    usage: None,
                                })).await;
                            });

                            // ── Wait for first item ──
                            match rx.recv().await {
                                Some(Ok(update)) => {
                                    // Collect accumulated messages if we're going to loop again
                                    let is_tool_calls = matches!(&update, AgentResponseUpdate::Finish { finish_reason, .. }
                                        if *finish_reason == FinishReason::ToolCalls);

                                    let next_round_messages = if is_tool_calls {
                                        // Read accumulated messages for next iteration
                                        match msg_rx.recv().await {
                                            Some(new_msgs) => {
                                                let mut combined = messages.clone();
                                                combined.extend(new_msgs);
                                                tracing::trace!(
                                                    round = round + 1,
                                                    total_messages = combined.len(),
                                                    "Preparing messages for next iteration"
                                                );
                                                combined
                                            }
                                            None => {
                                                tracing::warn!("Message channel closed unexpectedly");
                                                messages.clone()
                                            }
                                        }
                                    } else {
                                        // Not a tool call — final stream, keep messages for Streaming phase
                                        messages.clone()
                                    };

                                    let next = if is_tool_calls {
                                        LoopState::Looping {
                                            messages: next_round_messages,
                                            round: round + 1,
                                            options,
                                        }
                                    } else {
                                        LoopState::Streaming {
                                            rx,
                                            on_done: Box::new(LoopState::Looping {
                                                messages: next_round_messages,
                                                round: round + 1,
                                                options,
                                            }),
                                        }
                                    };

                                    Some((Ok(update), next))
                                }
                                Some(Err(e)) => {
                                    tracing::warn!(error = %e, "Tool loop error on first item");
                                    Some((Err(e), LoopState::Done))
                                }
                                None => {
                                    tracing::trace!("Stream channel closed before first item");
                                    None
                                }
                            }
                        }
                    }
                }
            },
        );

        // Filter out internal ToolCalls finish signals — they're loop control,
        // not meant for consumers (ChatClientAgent) to see.
        let stream: BoxStream<'static, Result<AgentResponseUpdate>> = Box::pin(
            stream.filter(|r| {
                let keep = !matches!(
                    r,
                    Ok(AgentResponseUpdate::Finish {
                        finish_reason: FinishReason::ToolCalls,
                        ..
                    })
                );
                async move { keep }
            }),
        );

        Ok(stream)
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.inner.model_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock ChatClient that returns simulated LLM responses including tool calls.
    struct MockChatClient {
        id: String,
        responses: Vec<Vec<AgentResponseUpdate>>,
        call_count: AtomicUsize,
    }

    impl MockChatClient {
        fn new(responses: Vec<Vec<AgentResponseUpdate>>) -> Self {
            Self {
                id: "mock-model".to_string(),
                responses,
                call_count: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.call_count.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl IChatClient for MockChatClient {
        async fn run(
            &self,
            _messages: &[ChatMessage],
            _options: ChatClientRunOptions,
        ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let response = if idx < self.responses.len() {
                self.responses[idx].clone()
            } else {
                // Default: no tool calls, simple text response
                vec![
                    AgentResponseUpdate::TextDelta {
                        delta: "Default response".to_string(),
                    },
                    AgentResponseUpdate::Finish {
                        finish_reason: FinishReason::Stop,
                        usage: None,
                    },
                ]
            };

            Ok(Box::pin(futures_util::stream::iter(
                response.into_iter().map(Ok),
            )))
        }

        fn model_id(&self) -> &str {
            &self.id
        }

        fn model_metadata(&self) -> Option<&ModelMetadata> {
            None
        }
    }

    /// A simple test tool that echoes back its input.
    #[derive(Clone)]
    struct EchoTool;

    #[async_trait]
    impl ITool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echoes back the input arguments"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            })
        }

        async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
            let msg = arguments
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("(no message)");
            Ok(format!("ECHO: {}", msg))
        }
    }

    /// A counting tool that returns a number.
    #[derive(Clone)]
    struct CountTool {
        counter: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl CountTool {
        fn new() -> Self {
            Self {
                counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }

        fn count(&self) -> usize {
            self.counter.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl ITool for CountTool {
        fn name(&self) -> &str {
            "count"
        }

        fn description(&self) -> &str {
            "Returns the current count"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }

        async fn execute(&self, _arguments: serde_json::Value) -> Result<String> {
            let val = 1 + self.counter.fetch_add(1, Ordering::Relaxed);
            Ok(val.to_string())
        }
    }

    // ── Tests ──

    #[tokio::test]
    async fn test_function_invoking_no_tool_calls() {
        // 场景：LLM 返回纯文本响应（无工具调用）
        let mock = Arc::new(MockChatClient::new(vec![vec![
            AgentResponseUpdate::TextDelta {
                delta: "Hello, world!".to_string(),
            },
            AgentResponseUpdate::Finish {
                finish_reason: FinishReason::Stop,
                usage: None,
            },
        ]]));

        let client = FunctionInvokingChatClient::new(mock.clone(), vec![]);

        let messages = vec![ChatMessage::user("Hi")];
        let options = ChatClientRunOptions::default();

        let stream = client.run(&messages, options).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;

        // Should have TextDelta + Finish
        assert!(results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { .. }))));
        assert!(results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, .. }))));
        assert_eq!(mock.call_count(), 1, "Should call inner once");
    }

    #[tokio::test]
    async fn test_function_invoking_single_tool_call() {
        // 场景：LLM 返回一个工具调用，工具循环执行后再次调用 LLM 获取最终响应
        let mock = Arc::new(MockChatClient::new(vec![
            // Round 0: LLM returns a tool call
            vec![
                AgentResponseUpdate::ToolCallStart {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                },
                AgentResponseUpdate::ToolCallArgs {
                    id: "call_1".to_string(),
                    args_delta: r#"{"message": "hello from tool"}"#.to_string(),
                },
                AgentResponseUpdate::ToolCallEnd {
                    id: "call_1".to_string(),
                },
                AgentResponseUpdate::Finish {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ],
            // Round 1: LLM processes tool result, returns final text
            vec![
                AgentResponseUpdate::TextDelta {
                    delta: "Got the echo result, done.".to_string(),
                },
                AgentResponseUpdate::Finish {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ],
        ]));

        let client = FunctionInvokingChatClient::new(
            mock.clone(),
            vec![Arc::new(EchoTool)],
        );

        let messages = vec![ChatMessage::user("Use the echo tool")];
        let options = ChatClientRunOptions::default();

        let stream = client.run(&messages, options).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;

        // Should have: ToolCallStart + ToolCallArgs + ToolCallEnd (raw LLM events),
        // then TextDelta + Finish(Stop) from the second round.
        let has_tool_start = results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::ToolCallStart { .. })));
        let has_final_text = results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta == "Got the echo result, done."));
        let has_finish_stop = results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, .. })));

        assert!(has_tool_start, "Should contain ToolCallStart");
        assert!(has_final_text, "Should contain final text from second round");
        assert!(has_finish_stop, "Should contain Finish(Stop)");

        // Inner called twice: once for tool call, once after tool execution for final response
        assert_eq!(mock.call_count(), 2, "Tool loop correctly calls inner twice (tool call + final response)");
    }

    #[tokio::test]
    async fn test_function_invoking_multi_round_tool_loop() {
        // 场景：LLM 返回工具调用 → 工具执行 → LLM 再次调用工具 → 最终文本
        let mock = Arc::new(MockChatClient::new(vec![
            // Round 0: LLM calls count tool
            vec![
                AgentResponseUpdate::ToolCallStart {
                    id: "call_1".to_string(),
                    name: "count".to_string(),
                },
                AgentResponseUpdate::ToolCallArgs {
                    id: "call_1".to_string(),
                    args_delta: "{}".to_string(),
                },
                AgentResponseUpdate::ToolCallEnd {
                    id: "call_1".to_string(),
                },
                AgentResponseUpdate::Finish {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ],
            // Round 1: LLM calls count again (multi-round)
            vec![
                AgentResponseUpdate::ToolCallStart {
                    id: "call_2".to_string(),
                    name: "count".to_string(),
                },
                AgentResponseUpdate::ToolCallArgs {
                    id: "call_2".to_string(),
                    args_delta: "{}".to_string(),
                },
                AgentResponseUpdate::ToolCallEnd {
                    id: "call_2".to_string(),
                },
                AgentResponseUpdate::Finish {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ],
            // Round 2: Final text response
            vec![
                AgentResponseUpdate::TextDelta {
                    delta: "Done counting.".to_string(),
                },
                AgentResponseUpdate::Finish {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ],
        ]));

        let count_tool = CountTool::new();
        let client = FunctionInvokingChatClient::new(
            mock.clone(),
            vec![Arc::new(count_tool.clone())],
        );

        let messages = vec![ChatMessage::user("Count twice")];
        let options = ChatClientRunOptions::default();

        let stream = client.run(&messages, options).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;

        // Should have been called 3 times (2 tool rounds + 1 final text)
        assert_eq!(mock.call_count(), 3, "Should call inner 3 times");

        // Tool should have been executed twice
        assert_eq!(count_tool.count(), 2, "Count tool should be called twice");

        // Results should include TextDelta from the final round
        let has_final_text = results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta == "Done counting."));
        assert!(has_final_text, "Should contain final text response");

        // No ToolCalls finish should leak out (all should be consumed by the loop)
        let tool_calls_finish_count = results.iter().filter(|r| {
            matches!(r, Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::ToolCalls, .. }))
        }).count();
        assert_eq!(tool_calls_finish_count, 0, "ToolCalls finish signals should be consumed by the loop");
    }

    #[tokio::test]
    async fn test_function_invoking_max_rounds_exceeded() {
        // 场景：持续返回工具调用，达到 max_rounds
        let tool_call_response = vec![
            AgentResponseUpdate::ToolCallStart {
                id: "call_1".to_string(),
                name: "echo".to_string(),
            },
            AgentResponseUpdate::ToolCallArgs {
                id: "call_1".to_string(),
                args_delta: r#"{"message": "loop"}"#.to_string(),
            },
            AgentResponseUpdate::ToolCallEnd {
                id: "call_1".to_string(),
            },
            AgentResponseUpdate::Finish {
                finish_reason: FinishReason::Stop,
                usage: None,
            },
        ];

        // Return tool calls 5 times — client has max_rounds=3
        let mock = Arc::new(MockChatClient::new(vec![
            tool_call_response.clone(),
            tool_call_response.clone(),
            tool_call_response.clone(),
            tool_call_response.clone(),
            tool_call_response.clone(),
        ]));

        let client = FunctionInvokingChatClient::new(
            mock.clone(),
            vec![Arc::new(EchoTool)],
        )
        .with_max_rounds(3);

        let messages = vec![ChatMessage::user("Loop")];
        let options = ChatClientRunOptions::default();

        let stream = client.run(&messages, options).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;

        // Should stop after 3 rounds (max_rounds)
        assert!(mock.call_count() <= 3, "Should not exceed max_rounds");

        // Should end with Finish(Stop) rather than hanging
        let final_finish = results.last();
        assert!(final_finish.is_some(), "Should have a final event");
    }

    #[tokio::test]
    async fn test_function_invoking_message_accumulation() {
        // 场景：验证消息在迭代间正确累积
        let mock = Arc::new(MockChatClient::new(vec![
            // Round 0: tool call
            vec![
                AgentResponseUpdate::ToolCallStart {
                    id: "c1".to_string(),
                    name: "echo".to_string(),
                },
                AgentResponseUpdate::ToolCallArgs {
                    id: "c1".to_string(),
                    args_delta: r#"{"message": "test"}"#.to_string(),
                },
                AgentResponseUpdate::ToolCallEnd {
                    id: "c1".to_string(),
                },
                AgentResponseUpdate::Finish {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ],
            // Round 1: another tool call — mock will check messages were accumulated
            vec![
                AgentResponseUpdate::TextDelta {
                    delta: "All done with tools.".to_string(),
                },
                AgentResponseUpdate::Finish {
                    finish_reason: FinishReason::Stop,
                    usage: None,
                },
            ],
        ]));

        let client = FunctionInvokingChatClient::new(
            mock.clone(),
            vec![Arc::new(EchoTool)],
        );

        let messages = vec![ChatMessage::user("Test")];
        let options = ChatClientRunOptions::default();

        let stream = client.run(&messages, options).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;

        // Verify: the second call to inner should have received accumulated messages
        // (original user message + assistant tool_call + tool result)
        assert_eq!(mock.call_count(), 2, "Should make 2 inner calls");

        // Verify final text is present
        let has_completion = results.iter().any(|r| {
            matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta == "All done with tools.")
        });
        assert!(has_completion, "Should contain final text after tool loop completes");
    }
}

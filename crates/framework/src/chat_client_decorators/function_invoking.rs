use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

use rust_agent_core::{
    AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage, FinishReason,
    IChatClient, ITool, MessageRole, ModelMetadata, ResponseMetadata, Result,
    ToolApprovalResponse, ToolCalledContent, ToolCall,
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
        /// Base messages + channel to receive tool results when loop continues
        on_done: Box<LoopState>,
        msg_rx: Option<mpsc::Receiver<Vec<ChatMessage>>>,
    },
    Done,
}

/// 按工具的 JSON Schema 验证参数是否合法。
///
/// 校验逻辑：
/// 1. 参数必须是一个 JSON 对象（含对空对象降级为字符串边界情况的包容处理）
/// 2. 所有 `required` 字段必须存在
/// 3. 每个字段的值类型必须与 `properties[<field>].type` 兼容
///
/// 返回 `Ok(())` 表示校验通过；`Err(message)` 给出人类可读的缺失/错误清单。
fn validate_against_schema(args: &serde_json::Value, schema: &serde_json::Value) -> std::result::Result<(), String> {
    let obj = match args {
        serde_json::Value::Object(o) => o,
        other => {
            // 包容处理：空字符串/空数组/基本类型 — 给一个明确提示
            return Err(format!(
                "Expected a JSON object with named parameters, but received: {}. \
                 Please use the format {{\"param1\": value1, \"param2\": value2}}.",
                match other {
                    serde_json::Value::String(s) if s.is_empty() => "an empty string",
                    serde_json::Value::String(s) => return Err(format!(
                        "Expected a JSON object, but received a bare string: \"{}\". \
                         Did you mean {{\"<parameter_name>\": \"{}\"}}? \
                         Check the schema below for the correct parameter names.",
                        s, s
                    )),
                    serde_json::Value::Array(_) => "an array",
                    serde_json::Value::Number(_) => "a number",
                    serde_json::Value::Bool(_) => "a boolean",
                    serde_json::Value::Null => "null",
                    _ => "an unexpected value type",
                }
            ));
        }
    };

    let mut errors: Vec<String> = Vec::new();

    // 1. 校验 required 字段
    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for field in required {
            let field_name = field.as_str().unwrap_or("");
            if !obj.contains_key(field_name) {
                errors.push(format!("Missing required field: \"{}\"", field_name));
            }
        }
    }

    // 2. 校验每个已提供字段的类型
    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
        for (field_name, field_value) in obj {
            if let Some(prop_schema) = properties.get(field_name) {
                let expected_type = prop_schema
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("string");

                let type_ok = match expected_type {
                    "string" => field_value.is_string(),
                    "number" | "integer" => field_value.is_number(),
                    "boolean" => field_value.is_boolean(),
                    "object" => field_value.is_object(),
                    "array" => field_value.is_array(),
                    _ => true, // 未知类型不校验
                };

                if !type_ok {
                    let actual = match field_value {
                        serde_json::Value::String(_) => "string",
                        serde_json::Value::Number(_) => "number",
                        serde_json::Value::Bool(_) => "boolean",
                        serde_json::Value::Array(_) => "array",
                        serde_json::Value::Object(_) => "object",
                        serde_json::Value::Null => "null",
                    };
                    errors.push(format!(
                        "Field \"{}\" has type \"{}\", but expected type \"{}\".",
                        field_name, actual, expected_type
                    ));
                }
            }
            // 未知字段不报错 — LLM 有时会发送额外字段，工具可以选择忽略
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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

                        LoopState::Streaming { mut rx, on_done, mut msg_rx } => {
                            match rx.recv().await {
                                Some(Ok(update)) => {
                                    if matches!(&update, AgentResponseUpdate::Finish { finish_reason, .. }
                                        if *finish_reason == FinishReason::ToolCalls)
                                    {
                                        tracing::trace!("Detected ToolCalls finish signal, transitioning to next loop iteration");
                                        // Read accumulated messages (built by the spawned task)
                                        if let Some(ref mut mr) = msg_rx {
                                            match mr.recv().await {
                                                Some(new_msgs) => {
                                                    // Clone the next state and merge accumulated messages
                                                    match *on_done {
                                                        LoopState::Looping { messages, round, options } => {
                                                            let mut combined = messages;
                                                            combined.extend(new_msgs);
                                                            tracing::trace!(round, total_messages = combined.len(), "Streaming→Looping with accumulated messages");
                                                            Some((Ok(update), LoopState::Looping { messages: combined, round, options }))
                                                        }
                                                        other => Some((Ok(update), other)),
                                                    }
                                                }
                                                None => {
                                                    tracing::warn!("Message channel closed unexpectedly");
                                                    Some((Ok(update), *on_done))
                                                }
                                            }
                                        } else {
                                            Some((Ok(update), *on_done))
                                        }
                                    } else {
                                        Some((Ok(update), LoopState::Streaming { rx, on_done, msg_rx }))
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
                            // ── Cancel check (before anything else) ──
                            if let Some(ref flag) = options.cancelled {
                                use std::sync::atomic::Ordering;
                                if flag.load(Ordering::Relaxed) {
                                    tracing::info!(round, "Agent run cancelled");
                                    let err_update = AgentResponseUpdate::Error {
                                        message: "Agent run cancelled".into(),
                                    };
                                    return Some((Ok(err_update), LoopState::Done));
                                }
                            }

                            // ── Approval resume: process pending approval responses ──
                            if !options.tool_approval_responses.is_empty() {
                                tracing::info!(
                                    response_count = options.tool_approval_responses.len(),
                                    "Resuming after approval pause"
                                );

                                let approval_map: std::collections::HashMap<&str, &ToolApprovalResponse> =
                                    options
                                        .tool_approval_responses
                                        .iter()
                                        .map(|r| (r.call_id.as_str(), r))
                                        .collect();

                                // Find pending tool_calls from the last assistant message
                                let pending: Vec<ToolCall> = messages
                                    .iter()
                                    .rev()
                                    .find(|m| {
                                        m.role == MessageRole::Assistant && m.tool_calls.is_some()
                                    })
                                    .and_then(|m| m.tool_calls.clone())
                                    .unwrap_or_default();

                                let mut next_messages = messages.clone();
                                for tc in &pending {
                                    let approved = approval_map
                                        .get(tc.id.as_str())
                                        .map(|r| r.approved)
                                        .unwrap_or(false);

                                    if approved {
                                        let args = match &tc.arguments {
                                            serde_json::Value::String(s) => {
                                                serde_json::from_str(s)
                                                    .unwrap_or(serde_json::Value::Null)
                                            }
                                            other => other.clone(),
                                        };
                                        let result = match tools
                                            .iter()
                                            .find(|t| t.name() == tc.name)
                                        {
                                            Some(tool) => match tool.execute(args).await {
                                                Ok(output) => output,
                                                Err(e) => format!("Error: {}", e),
                                            },
                                            None => {
                                                format!("Tool '{}' not found", tc.name)
                                            }
                                        };
                                        next_messages.push(ChatMessage {
                                            role: MessageRole::Tool,
                                            content: result,
                                            name: Some(tc.name.clone()),
                                            tool_calls: None,
                                            tool_call_id: Some(tc.id.clone()),
                                            source: None,
                                        });
                                    } else {
                                        let reason = approval_map
                                            .get(tc.id.as_str())
                                            .and_then(|r| r.reason.as_deref())
                                            .unwrap_or("User denied");
                                        next_messages.push(ChatMessage {
                                            role: MessageRole::Tool,
                                            content: format!("Rejected: {}", reason),
                                            name: Some(tc.name.clone()),
                                            tool_calls: None,
                                            tool_call_id: Some(tc.id.clone()),
                                            source: None,
                                        });
                                    }
                                }

                                let mut opts = options.clone();
                                opts.tool_approval_responses.clear();
                                return Some((
                                    Ok(AgentResponseUpdate::TextDelta {
                                        delta: String::new(),
                                    }),
                                    LoopState::Looping {
                                        messages: next_messages,
                                        round: round + 1,
                                        options: opts,
                                    },
                                ));
                            }

                            if round >= max_rounds {
                                tracing::warn!(
                                    round,
                                    max_rounds,
                                    "Tool loop reached max rounds, terminating"
                                );
                                let err_update = AgentResponseUpdate::Finish {
                                    finish_reason: FinishReason::MaxRounds,
                                    usage: None,
                                };
                                return Some((Ok(err_update), LoopState::Done));
                            }

                            tracing::info!(
                                round,
                                message_count = messages.len(),
                                "Tool loop iteration starting"
                            );

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

                            let (tx, mut rx) = mpsc::channel::<Result<AgentResponseUpdate>>(256);
                            let (msg_tx, mut msg_rx) = mpsc::channel::<Vec<ChatMessage>>(1);

                            // Merge provider-injected tools with statically-registered tools.
                            // Follows MAF's pattern where FunctionInvokingChatClient resolves
                            // tools from ChatOptions.Tools at execution time, ensuring that
                            // tools injected by ContextProviders (e.g. load_skill) are
                            // executable, not just sent as schemas to the LLM.
                            //
                            // Dedup by name: statically-registered tools take priority,
                            // except ApprovalRequiredTool wrappers which replace
                            // their non-approval counterparts.
                            let mut combined: Vec<Arc<dyn ITool>> = (*tools).clone();
                            let mut seen: std::collections::HashSet<String> = combined
                                .iter()
                                .map(|t| t.name().to_string())
                                .collect();
                            for pt in &options.provider_tools {
                                if seen.contains(pt.name()) {
                                    // ApprovalRequiredTool replaces existing non-approval tool
                                    if pt.requires_approval() {
                                        combined.retain(|t| t.name() != pt.name());
                                        combined.push(Arc::clone(pt));
                                        tracing::debug!(
                                            provider_tool = pt.name(),
                                            "ApprovalRequiredTool replaces existing tool"
                                        );
                                    } else {
                                        tracing::debug!(
                                            provider_tool = pt.name(),
                                            "Provider tool skipped — already registered"
                                        );
                                    }
                                } else {
                                    seen.insert(pt.name().to_string());
                                    combined.push(Arc::clone(pt));
                                }
                            }
                            let tools_for_execution = Arc::new(combined);

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
                                                    tracing::trace!(tool_name = %name, call_id = %id, "ToolCallStart received");
                                                    has_tool_calls = true;
                                                    // 并行 tool call：先提交上一个未 End 的调用
                                                    if let Some(prev) = current_tool_args.take() {
                                                        if !prev.id.is_empty() && !prev.name.is_empty() {
                                                            tool_calls.push(prev);
                                                        }
                                                    }
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
                                                    tracing::trace!(call_id = %id, args_delta_len = args_delta.len(), "ToolCallArgs received");
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
                                                    tracing::trace!(call_id = %id, "ToolCallEnd received — arguments complete");
                                                    if let Some(tc) = current_tool_args.take() {
                                                        if tc.id == *id {
                                                            tracing::trace!(tool_name = %tc.name, call_id = %tc.id, args_len = tc.arguments.len(), "Tool call finalized with args: {}", tc.arguments);
                                                            tool_calls.push(tc);
                                                        }
                                                    }
                                                    if tx.send(Ok(update)).await.is_err() {
                                                        tracing::trace!("Stream consumer dropped (ToolCallEnd)");
                                                        return;
                                                    }
                                                }
                                                AgentResponseUpdate::ToolCallDelta { index, id, name, arguments_delta } => {
                                                    tracing::trace!(index, "ToolCallDelta received (legacy format)");
                                                    has_tool_calls = true;
                                                    let idx = *index;
                                                    while tool_calls.len() <= idx {
                                                        tool_calls.push(AccumulatedToolCall::default());
                                                    }
                                                    if let Some(ref id) = id { tool_calls[idx].id = id.clone(); }
                                                    if let Some(ref name) = name { tool_calls[idx].name = name.clone(); }
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

                                // ── 收尾：Stream 结束时，如果 current_tool_args 仍有数据 ──
                                // 说明 ToolCallStart 已到达但 ToolCallEnd 未到达（流提前终止
                                // 或提供商的 streaming 格式异常）。此时需将未完成的 tool call
                                // 提交到执行队列，避免静默丢失工具调用。
                                if let Some(dangling) = current_tool_args.take() {
                                    if !dangling.id.is_empty() && !dangling.name.is_empty() {
                                        tracing::warn!(
                                            tool_name = %dangling.name,
                                            call_id = %dangling.id,
                                            args_len = dangling.arguments.len(),
                                            args_preview = %if dangling.arguments.len() > 200 {
                                                format!("{}...", &dangling.arguments[..200])
                                            } else {
                                                dangling.arguments.clone()
                                            },
                                            "Stream ended with unfinished tool call — finalizing"
                                        );
                                        has_tool_calls = true;
                                        tool_calls.push(dangling);
                                    }
                                }

                                // 丢弃无 id/name 的占位槽位（ToolCallDelta 按 index 预分配时可能产生）
                                tool_calls.retain(|tc| !tc.id.is_empty() && !tc.name.is_empty());

                                if !has_tool_calls || tool_calls.is_empty() {
                                    tracing::info!("No tool calls detected — emitting final Finish(Stop)");
                                    let _ = tx.send(Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None })).await;
                                    return;
                                }

                                // ── Approval gate: if any tool requires approval, pause and wait ──
                                let any_requires_approval = tool_calls.iter().any(|tc| {
                                    tools_for_execution
                                        .iter()
                                        .any(|t| t.name() == tc.name && t.requires_approval())
                                });

                                if any_requires_approval {
                                    tracing::info!(
                                        tool_count = tool_calls.len(),
                                        "Tool requires approval — emitting ToolApprovalRequest events"
                                    );

                                    for tc in &tool_calls {
                                        let tool = tools_for_execution
                                            .iter()
                                            .find(|t| t.name() == tc.name);
                                        let desc = tool
                                            .map(|t| t.description().to_string())
                                            .unwrap_or_default();
                                        let args: serde_json::Value =
                                            serde_json::from_str(&tc.arguments)
                                                .unwrap_or(serde_json::Value::Null);

                                        if tx
                                            .send(Ok(AgentResponseUpdate::ToolApprovalRequest {
                                                call_id: tc.id.clone(),
                                                name: tc.name.clone(),
                                                arguments: args,
                                                description: desc,
                                            }))
                                            .await
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }

                                    // Persist assistant(tool_calls) to accumulated messages
                                    let mut next_messages = Vec::new();
                                    let assistant_tool_msg = ChatMessage {
                                        role: MessageRole::Assistant,
                                        content: text_delta.clone(),
                                        name: None,
                                        tool_calls: Some(
                                            tool_calls
                                                .iter()
                                                .map(|tc| ToolCall {
                                                    id: tc.id.clone(),
                                                    name: tc.name.clone(),
                                                    arguments: serde_json::Value::String(
                                                        tc.arguments.clone(),
                                                    ),
                                                })
                                                .collect(),
                                        ),
                                        tool_call_id: None,
                                        source: None,
                                    };
                                    next_messages.push(assistant_tool_msg);
                                    let _ = msg_tx.send(next_messages).await;

                                    // End stream — wait for caller to approve before resuming
                                    let _ = tx
                                        .send(Ok(AgentResponseUpdate::Finish {
                                            finish_reason: FinishReason::AwaitingApproval,
                                            usage: None,
                                        }))
                                        .await;
                                    return;
                                }

                                tracing::info!(tool_count = tool_calls.len(), "Executing tools");

                                let meta = ResponseMetadata {
                                    agent_id: None, model_id: None, executor_id: None,
                                    timestamp: Utc::now(), properties: Default::default(),
                                };

                                let tool_futures: Vec<_> = tool_calls.iter().map(|tc| {
                                    let tc = tc.clone();
                                    let tools = Arc::clone(&tools_for_execution);
                                    let meta = meta.clone();
                                    async move {
                                        tracing::trace!(tool_name = %tc.name, call_id = %tc.id, args = %tc.arguments, "Executing tool");

                                        // ── 处理空参数：当 LLM 发送的参数字符串完全为空时（非 "{}"），提前返回 schema 信息 ──
                                        let args_trimmed = tc.arguments.trim();
                                        if args_trimmed.is_empty() {
                                            if let Some(tool) = tools.iter().find(|t| t.name() == tc.name) {
                                                let schema = tool.parameters();
                                                let msg = format!(
                                                    "Tool '{}' was called without any arguments. Expected schema: {}. Please provide all required fields.",
                                                    tc.name, schema
                                                );
                                                tracing::warn!(tool_name = %tc.name, call_id = %tc.id, "Empty tool call arguments (empty string)");
                                                return ToolCalledContent {
                                                    meta, call_id: tc.id.clone(), result: None, error: Some(msg),
                                                };
                                            }
                                        }

                                        let args_value = match serde_json::from_str(&tc.arguments) {
                                            Ok(v) => v,
                                            Err(e) => {
                                                tracing::warn!(
                                                    tool_name = %tc.name,
                                                    call_id = %tc.id,
                                                    error = %e,
                                                    raw_args = %tc.arguments,
                                                    "Failed to parse tool call arguments as JSON"
                                                );
                                                // JSON 解析失败时直接返回错误，不再静默降级为空对象。
                                                // 带上原始参数和 schema，让 LLM 能自我纠正。
                                                let msg = format!(
                                                    "Tool '{}' was called with invalid JSON arguments: {}\n\
                                                     Parse error: {}\n\
                                                     Please provide valid JSON with all required fields.",
                                                    tc.name, tc.arguments, e
                                                );
                                                if let Some(tool) = tools.iter().find(|t| t.name() == tc.name) {
                                                    let schema = tool.parameters();
                                                    return ToolCalledContent {
                                                        meta, call_id: tc.id.clone(), result: None,
                                                        error: Some(format!("{} Expected schema: {}", msg, schema)),
                                                    };
                                                }
                                                return ToolCalledContent {
                                                    meta, call_id: tc.id.clone(), result: None, error: Some(msg),
                                                };
                                            }
                                        };

                                        // ── Schema 验证：在工具执行前校验参数完整性 ──
                                        // 不依赖工具内部的 serde 反序列化报错（不同工具报错格式不一致），
                                        // 而是统一在此处按声明的 JSON Schema 做前置校验。
                                        if let Some(tool) = tools.iter().find(|t| t.name() == tc.name) {
                                            let schema = tool.parameters();
                                            if let Err(validation_err) = validate_against_schema(&args_value, &schema) {
                                                tracing::warn!(
                                                    tool_name = %tc.name,
                                                    call_id = %tc.id,
                                                    error = %validation_err,
                                                    raw_args = %tc.arguments,
                                                    "Tool call arguments failed schema validation"
                                                );
                                                let msg = format!(
                                                    "Tool '{}' was called with invalid or incomplete arguments.\n\
                                                     You sent: {}\n\
                                                     Problem: {}\n\
                                                     Expected schema: {}\n\
                                                     Please fix your arguments and retry.",
                                                    tc.name, tc.arguments, validation_err, schema
                                                );
                                                return ToolCalledContent {
                                                    meta, call_id: tc.id.clone(), result: None, error: Some(msg),
                                                };
                                            }
                                        }

                                        match tools.iter().find(|t| t.name() == tc.name) {
                                            Some(tool) => match tool.execute(args_value).await {
                                                Ok(output) => {
                                                    tracing::trace!(tool_name = %tc.name, call_id = %tc.id, has_error = false, output_len = output.len(), "Tool execution succeeded");
                                                    ToolCalledContent { meta, call_id: tc.id.clone(), result: Some(output), error: None }
                                                }
                                                Err(e) => {
                                                    tracing::warn!(tool_name = %tc.name, call_id = %tc.id, error = %e, "Tool execution failed");
                                                    let schema = tool.parameters();
                                                    let msg = format!(
                                                        "Tool '{}' execution failed: {}\n\
                                                         You called with arguments: {}\n\
                                                         Expected schema: {}\n\
                                                         Please fix your arguments and retry.",
                                                        tc.name, e, tc.arguments, schema
                                                    );
                                                    ToolCalledContent { meta, call_id: tc.id.clone(), result: None, error: Some(msg) }
                                                }
                                            },
                                            None => {
                                                tracing::warn!(tool_name = %tc.name, call_id = %tc.id, "Tool not found in registry");
                                                ToolCalledContent { meta, call_id: tc.id.clone(), result: None, error: Some(format!("Tool '{}' not found", tc.name)) }
                                            }
                                        }
                                    }
                                }).collect::<Vec<_>>();

                                let results = futures_util::future::join_all(tool_futures).await;

                                let mut next_messages = Vec::new();
                                let assistant_tool_msg = ChatMessage {
                                    role: MessageRole::Assistant, content: text_delta.clone(), name: None,
                                    tool_calls: Some(tool_calls.iter().map(|tc| ToolCall {
                                        id: tc.id.clone(), name: tc.name.clone(),
                                        arguments: serde_json::Value::String(tc.arguments.clone()),
                                    }).collect()),
                                    tool_call_id: None, source: None,
                                };
                                next_messages.push(assistant_tool_msg);

                                for (i, result) in results.iter().enumerate() {
                                    let call_id = &tool_calls[i].id;
                                    let content = result.result.clone()
                                        .or_else(|| result.error.clone())
                                        .unwrap_or_default();
                                    next_messages.push(ChatMessage {
                                        role: MessageRole::Tool, content,
                                        name: Some(tool_calls[i].name.clone()),
                                        tool_calls: None, tool_call_id: Some(call_id.clone()), source: None,
                                    });
                                }

                                tracing::trace!(assistant_msg_count = 1, tool_result_count = results.len(), "Built accumulated messages for next iteration");
                                let _ = msg_tx.send(next_messages).await;

                                for (i, result) in results.into_iter().enumerate() {
                                    let call_id = tool_calls[i].id.clone();
                                    tracing::trace!(call_id = %call_id, has_error = result.error.is_some(), "Emitting tool result event");
                                    if tx.send(Ok(AgentResponseUpdate::ToolCalled {
                                        id: call_id,
                                        result: result.result,
                                        error: result.error,
                                    })).await.is_err() {
                                        tracing::trace!("Stream consumer dropped during result emission");
                                        return;
                                    }
                                }

                                tracing::trace!("Emitting ToolCalls finish signal for loop continuation");
                                let _ = tx.send(Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::ToolCalls, usage: None })).await;
                            });

                            match rx.recv().await {
                                Some(Ok(update)) => {
                                    let is_tool_calls = matches!(&update, AgentResponseUpdate::Finish { finish_reason, .. }
                                        if *finish_reason == FinishReason::ToolCalls);

                                    let next = if is_tool_calls {
                                        // Rare: first item already is ToolCalls — read accumulated messages now
                                        let combined = match msg_rx.recv().await {
                                            Some(new_msgs) => {
                                                let mut c = messages.clone();
                                                c.extend(new_msgs);
                                                tracing::trace!(round = round + 1, total_messages = c.len(), "First-item ToolCalls: preparing messages for next iteration");
                                                c
                                            }
                                            None => {
                                                tracing::warn!("Message channel closed unexpectedly");
                                                messages.clone()
                                            }
                                        };
                                        LoopState::Looping { messages: combined, round: round + 1, options }
                                    } else {
                                        // Normal case: first item is ToolCallStart/etc.
                                        // Defer msg_rx to Streaming state — it will read accumulated messages
                                        // when it detects Finish(ToolCalls)
                                        LoopState::Streaming {
                                            rx,
                                            on_done: Box::new(LoopState::Looping {
                                                messages: messages.clone(),
                                                round: round + 1,
                                                options,
                                            }),
                                            msg_rx: Some(msg_rx),
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

        let stream: BoxStream<'static, Result<AgentResponseUpdate>> = Box::pin(
            stream.filter(|r| {
                let keep = !matches!(r, Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::ToolCalls, .. }));
                async move { keep }
            }),
        );

        Ok(stream)
    }

    fn model_id(&self) -> &str { self.inner.model_id() }
    fn model_metadata(&self) -> Option<&ModelMetadata> { self.inner.model_metadata() }
    fn inner_client(&self) -> Option<&Arc<dyn IChatClient>> { Some(&self.inner) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MockChatClient {
        id: String,
        responses: Vec<Vec<AgentResponseUpdate>>,
        call_count: AtomicUsize,
    }

    impl MockChatClient {
        fn new(responses: Vec<Vec<AgentResponseUpdate>>) -> Self {
            Self { id: "mock-model".to_string(), responses, call_count: AtomicUsize::new(0) }
        }
        fn call_count(&self) -> usize { self.call_count.load(Ordering::Relaxed) }
    }

    #[async_trait]
    impl IChatClient for MockChatClient {
        async fn run(&self, _messages: &[ChatMessage], _options: ChatClientRunOptions) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let response = if idx < self.responses.len() {
                self.responses[idx].clone()
            } else {
                vec![
                    AgentResponseUpdate::TextDelta { delta: "Default response".to_string() },
                    AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
                ]
            };
            Ok(Box::pin(futures_util::stream::iter(response.into_iter().map(Ok))))
        }
        fn model_id(&self) -> &str { &self.id }
        fn model_metadata(&self) -> Option<&ModelMetadata> { None }
    }

    #[derive(Clone)]
    struct EchoTool;
    #[async_trait]
    impl ITool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "Echoes back the input arguments" }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {"message": {"type": "string"}}, "required": ["message"]})
        }
        async fn execute(&self, arguments: serde_json::Value) -> Result<String> {
            let msg = arguments.get("message").and_then(|v| v.as_str()).unwrap_or("(no message)");
            Ok(format!("ECHO: {}", msg))
        }
    }

    #[derive(Clone)]
    struct CountTool {
        counter: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl CountTool {
        fn new() -> Self { Self { counter: Arc::new(std::sync::atomic::AtomicUsize::new(0)) } }
        fn count(&self) -> usize { self.counter.load(Ordering::Relaxed) }
    }
    #[async_trait]
    impl ITool for CountTool {
        fn name(&self) -> &str { "count" }
        fn description(&self) -> &str { "Returns the current count" }
        fn parameters(&self) -> serde_json::Value { serde_json::json!({"type": "object", "properties": {}}) }
        async fn execute(&self, _arguments: serde_json::Value) -> Result<String> {
            let val = 1 + self.counter.fetch_add(1, Ordering::Relaxed);
            Ok(val.to_string())
        }
    }

    #[tokio::test]
    async fn test_function_invoking_no_tool_calls() {
        let mock = Arc::new(MockChatClient::new(vec![vec![
            AgentResponseUpdate::TextDelta { delta: "Hello, world!".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ]]));
        let client = FunctionInvokingChatClient::new(mock.clone(), vec![]);
        let stream = client.run(&[ChatMessage::user("Hi")], ChatClientRunOptions::default()).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;
        assert!(results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { .. }))));
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn test_function_invoking_single_tool_call() {
        let mock = Arc::new(MockChatClient::new(vec![
            vec![
                AgentResponseUpdate::ToolCallStart { id: "call_1".to_string(), name: "echo".to_string() },
                AgentResponseUpdate::ToolCallArgs { id: "call_1".to_string(), args_delta: r#"{"message": "hello from tool"}"#.to_string() },
                AgentResponseUpdate::ToolCallEnd { id: "call_1".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ],
            vec![
                AgentResponseUpdate::TextDelta { delta: "Got the echo result, done.".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ],
        ]));
        let client = FunctionInvokingChatClient::new(mock.clone(), vec![Arc::new(EchoTool)]);
        let stream = client.run(&[ChatMessage::user("Use the echo tool")], ChatClientRunOptions::default()).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;
        assert!(results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::ToolCallStart { .. }))));
        assert!(results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta == "Got the echo result, done.")));
        assert_eq!(mock.call_count(), 2);
    }

    #[tokio::test]
    async fn test_function_invoking_multi_round_tool_loop() {
        let mock = Arc::new(MockChatClient::new(vec![
            vec![
                AgentResponseUpdate::ToolCallStart { id: "call_1".to_string(), name: "count".to_string() },
                AgentResponseUpdate::ToolCallArgs { id: "call_1".to_string(), args_delta: "{}".to_string() },
                AgentResponseUpdate::ToolCallEnd { id: "call_1".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ],
            vec![
                AgentResponseUpdate::ToolCallStart { id: "call_2".to_string(), name: "count".to_string() },
                AgentResponseUpdate::ToolCallArgs { id: "call_2".to_string(), args_delta: "{}".to_string() },
                AgentResponseUpdate::ToolCallEnd { id: "call_2".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ],
            vec![
                AgentResponseUpdate::TextDelta { delta: "Done counting.".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ],
        ]));
        let count_tool = CountTool::new();
        let client = FunctionInvokingChatClient::new(mock.clone(), vec![Arc::new(count_tool.clone())]);
        let stream = client.run(&[ChatMessage::user("Count twice")], ChatClientRunOptions::default()).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;
        assert_eq!(mock.call_count(), 3);
        assert_eq!(count_tool.count(), 2);
        assert!(results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta == "Done counting.")));
        let tf = results.iter().filter(|r| matches!(r, Ok(AgentResponseUpdate::Finish { finish_reason: FinishReason::ToolCalls, .. }))).count();
        assert_eq!(tf, 0);
    }

    #[tokio::test]
    async fn test_function_invoking_max_rounds_exceeded() {
        let tr = vec![
            AgentResponseUpdate::ToolCallStart { id: "call_1".to_string(), name: "echo".to_string() },
            AgentResponseUpdate::ToolCallArgs { id: "call_1".to_string(), args_delta: r#"{"message": "loop"}"#.to_string() },
            AgentResponseUpdate::ToolCallEnd { id: "call_1".to_string() },
            AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
        ];
        let mock = Arc::new(MockChatClient::new(vec![tr.clone(), tr.clone(), tr.clone(), tr.clone(), tr.clone()]));
        let client = FunctionInvokingChatClient::new(mock.clone(), vec![Arc::new(EchoTool)]).with_max_rounds(3);
        let stream = client.run(&[ChatMessage::user("Loop")], ChatClientRunOptions::default()).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;
        assert!(mock.call_count() <= 3);
        assert!(results.last().is_some());
    }

    #[tokio::test]
    async fn test_function_invoking_message_accumulation() {
        let mock = Arc::new(MockChatClient::new(vec![
            vec![
                AgentResponseUpdate::ToolCallStart { id: "c1".to_string(), name: "echo".to_string() },
                AgentResponseUpdate::ToolCallArgs { id: "c1".to_string(), args_delta: r#"{"message": "test"}"#.to_string() },
                AgentResponseUpdate::ToolCallEnd { id: "c1".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ],
            vec![
                AgentResponseUpdate::TextDelta { delta: "All done with tools.".to_string() },
                AgentResponseUpdate::Finish { finish_reason: FinishReason::Stop, usage: None },
            ],
        ]));
        let client = FunctionInvokingChatClient::new(mock.clone(), vec![Arc::new(EchoTool)]);
        let stream = client.run(&[ChatMessage::user("Test")], ChatClientRunOptions::default()).await.expect("run should succeed");
        let results: Vec<_> = stream.collect().await;
        assert_eq!(mock.call_count(), 2);
        assert!(results.iter().any(|r| matches!(r, Ok(AgentResponseUpdate::TextDelta { ref delta }) if delta == "All done with tools.")));
    }
}

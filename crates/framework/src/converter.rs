use std::collections::HashMap;
use std::collections::HashSet;

use chrono::Utc;
use rust_agent_core::{
    AgentId, AgentResponseResult, AgentResponseUpdate, AgentRunOptions, ArgsEvent, Content,
    ErrorContent, Event, FinishReason, ReasoningContent, ResponseMetadata,
    StreamingArgsParser, TextContent, ToolCallArgsContent, ToolCallArgsParsedContent,
    ToolCallArgsProgressContent, ToolCallEndContent, ToolCallStartContent,
    ToolCalledContent, ToolCallingContent, Usage, UsageContent,
};

/// Accumulator for streaming tool call deltas, keyed by call_id for parallel tool call support.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    name: Option<String>,
    args: String,
    /// Whether a ToolCallStart content has already been emitted for this call.
    /// Prevents duplicate start events when the transport layer provides both
    /// explicit ToolCallStart events and legacy ToolCallDelta decomposition.
    start_emitted: bool,
}

/// 将 AgentResponseUpdate（内部 SSE 层）转换为 AgentResponseResult（公共 API）。
///
/// 通过以 call_id（String）而非位置索引为键的累加器，支持并行工具调用。
/// 对于旧版 ToolCallDelta 事件，累加器将其分解为 ToolCallStart / ToolCallArgs / ToolCallEnd 生命周期内容变体。
pub struct AgentResponseConverter {
    agent_id: AgentId,
    model_id: Option<String>,
    executor_id: String,
    properties: HashMap<String, serde_json::Value>,
    // Internal state
    /// Active tool call accumulators keyed by call_id (supports parallel calls).
    tool_accumulators: HashMap<String, ToolCallAccumulator>,
    /// Maps tool call index → real call_id. Resolves id=None in subsequent
    /// ToolCallDelta events: after the first delta establishes the call ID,
    /// later deltas with id=None are routed to the correct accumulator.
    index_to_call_id: HashMap<usize, String>,
    /// Set of call_ids that have received ToolCallEnd — prevents duplicate End emission.
    ended_calls: HashSet<String>,
    /// Per-call streaming JSON parsers for incremental argument parsing.
    /// When ToolCallArgs deltas arrive, they are fed into the parser which
    /// emits ToolCallArgsParsed / ToolCallArgsProgress content in real time.
    args_parsers: HashMap<String, StreamingArgsParser>,
    response_id: Option<String>,
    response_model: Option<String>,
}

impl AgentResponseConverter {
    pub fn new(agent_id: AgentId, executor_id: String, options: &AgentRunOptions) -> Self {
        Self {
            agent_id,
            model_id: None,
            executor_id,
            properties: options.properties.clone(),
            tool_accumulators: HashMap::new(),
            index_to_call_id: HashMap::new(),
            ended_calls: HashSet::new(),
            args_parsers: HashMap::new(),
            response_id: None,
            response_model: None,
        }
    }

    /// Build a ResponseMetadata for each content/event
    fn build_meta(&self) -> ResponseMetadata {
        ResponseMetadata {
            agent_id: Some(self.agent_id.clone()),
            model_id: self.model_id.clone(),
            executor_id: Some(self.executor_id.clone()),
            timestamp: Utc::now(),
            properties: self.properties.clone(),
        }
    }

    /// 消费单个 AgentResponseUpdate，生成内容和事件向量。
    ///
    /// 对于流式工具调用生命周期（ToolCallStart → ToolCallArgs … → ToolCallEnd），
    /// 对应内容变体在流式传输时立即发出，使下游消费者能实时响应每个生命周期事件。
    ///
    /// 对于旧版 `ToolCallDelta`，转换器将其分解为三个生命周期内容变体（start/args/end），
    /// 使消费者获得一致的流式 API。
    pub fn consume(&mut self, update: AgentResponseUpdate) -> ConvertOutput {
        let mut contents = Vec::new();
        let events = Vec::new();

        match update {
            AgentResponseUpdate::TextDelta { delta } => {
                if !delta.is_empty() {
                    contents.push(Content::Text(TextContent {
                        meta: self.build_meta(),
                        delta,
                    }));
                }
            }
            AgentResponseUpdate::ReasoningDelta { delta } => {
                if !delta.is_empty() {
                    contents.push(Content::Reasoning(ReasoningContent {
                        meta: self.build_meta(),
                        delta,
                    }));
                }
            }

            // ── New explicit lifecycle events (preferred transport format) ──
            AgentResponseUpdate::ToolCallStart { id, name } => {
                let acc = self.tool_accumulators.entry(id.clone()).or_default();
                acc.name = Some(name.clone());
                acc.start_emitted = true;

                contents.push(Content::ToolCallStart(ToolCallStartContent {
                    meta: self.build_meta(),
                    call_id: id,
                    name,
                }));
            }
            AgentResponseUpdate::ToolCallArgs { id, args_delta } => {
                if args_delta.is_empty() {
                    return ConvertOutput {
                        contents,
                        events,
                    };
                }

                let acc = self.tool_accumulators.entry(id.clone()).or_default();
                // If we haven't seen a ToolCallStart for this call_id yet,
                // we can't emit args (tool name unknown). Accumulate silently.
                if acc.name.is_some() {
                    acc.args.push_str(&args_delta);
                    contents.push(Content::ToolCallArgs(ToolCallArgsContent {
                        meta: self.build_meta(),
                        call_id: id.clone(),
                        args_delta: args_delta.clone(),
                    }));

                    // Feed into streaming JSON parser for incremental arg progress
                    let parser = self.args_parsers.entry(id.clone()).or_default();
                    parser.push_bytes(args_delta.as_bytes());
                    let parse_events = parser.poll(&id);
                    for ev in parse_events {
                        match ev {
                            ArgsEvent::Parsed {
                                id: call_id,
                                name,
                                value,
                            } => {
                                contents.push(Content::ToolCallArgsParsed(
                                    ToolCallArgsParsedContent {
                                        meta: self.build_meta(),
                                        call_id,
                                        name,
                                        value,
                                    },
                                ));
                            }
                            ArgsEvent::Progress {
                                id: call_id,
                                name,
                                received,
                                value,
                            } => {
                                contents.push(Content::ToolCallArgsProgress(
                                    ToolCallArgsProgressContent {
                                        meta: self.build_meta(),
                                        call_id,
                                        name,
                                        received,
                                        value,
                                    },
                                ));
                            }
                        }
                    }
                } else {
                    // Accumulate args even without a name (may get name later via legacy delta).
                    acc.args.push_str(&args_delta);
                }
            }
            AgentResponseUpdate::ToolCallEnd { id } => {
                if self.ended_calls.contains(&id) {
                    // Duplicate end — ignore
                    return ConvertOutput {
                        contents,
                        events,
                    };
                }
                self.ended_calls.insert(id.clone());

                let emit_end = {
                    let acc = self.tool_accumulators.get(&id);
                    // Only emit End if we previously emitted a Start
                    acc.map(|a| a.start_emitted).unwrap_or(false)
                };

                if emit_end {
                    contents.push(Content::ToolCallEnd(ToolCallEndContent {
                        meta: self.build_meta(),
                        call_id: id,
                    }));
                }
                // If no Start was emitted (e.g., only End arrived), we skip emitting
                // End now. The complete ToolCallingContent will be emitted in flush_tool_calls().
            }

            // ── Legacy ToolCallDelta — decompose into lifecycle content variants ──
            AgentResponseUpdate::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                // Resolve call_id: prefer explicit id, then fall back to
                // the index→call_id mapping (subsequent deltas without id),
                // and finally a placeholder for very first unnamed delta.
                let call_id = id.as_ref().or_else(|| {
                    self.index_to_call_id.get(&index)
                }).cloned().unwrap_or_else(|| format!("__tc_{}", index));

                // Remember the mapping for future deltas without id
                if let Some(ref real_id) = id {
                    self.index_to_call_id.insert(index, real_id.clone());
                }

                // Pre-compute meta to avoid borrow conflict with tool_accumulators entry
                let meta = self.build_meta();

                let acc = self.tool_accumulators.entry(call_id.clone()).or_default();

                // Emit ToolCallStart if this is the first time we see this call and
                // we have a name (or this is the first delta with a name).
                if let Some(ref new_name) = name {
                    if !acc.start_emitted {
                        acc.name = Some(new_name.clone());
                        acc.start_emitted = true;

                        contents.push(Content::ToolCallStart(ToolCallStartContent {
                            meta: meta.clone(),
                            call_id: call_id.clone(),
                            name: new_name.clone(),
                        }));
                    }
                } else if !acc.start_emitted && acc.name.is_some() {
                    // We already know the name from a previous delta — emit Start now
                    acc.start_emitted = true;
                    let acc_name = acc.name.clone().expect("ToolCallDelta acc.name was checked with is_some() above");
                    contents.push(Content::ToolCallStart(ToolCallStartContent {
                        meta: meta.clone(),
                        call_id: call_id.clone(),
                        name: acc_name,
                    }));
                }

                // Emit args delta if the tool name is known
                if !arguments_delta.is_empty() && acc.name.is_some() {
                    acc.args.push_str(&arguments_delta);
                    contents.push(Content::ToolCallArgs(ToolCallArgsContent {
                        meta: meta.clone(),
                        call_id: call_id.clone(),
                        args_delta: arguments_delta.clone(),
                    }));

                    // Feed into streaming JSON parser for incremental arg progress
                    let parser = self.args_parsers.entry(call_id.clone()).or_default();
                    parser.push_bytes(arguments_delta.as_bytes());
                    let parse_events = parser.poll(&call_id);
                    for ev in parse_events {
                        match ev {
                            ArgsEvent::Parsed {
                                id: cid,
                                name: pname,
                                value,
                            } => {
                                contents.push(Content::ToolCallArgsParsed(
                                    ToolCallArgsParsedContent {
                                        meta: meta.clone(),
                                        call_id: cid,
                                        name: pname,
                                        value,
                                    },
                                ));
                            }
                            ArgsEvent::Progress {
                                id: cid,
                                name: pname,
                                received,
                                value,
                            } => {
                                contents.push(Content::ToolCallArgsProgress(
                                    ToolCallArgsProgressContent {
                                        meta: meta.clone(),
                                        call_id: cid,
                                        name: pname,
                                        received,
                                        value,
                                    },
                                ));
                            }
                        }
                    }
                } else {
                    // Accumulate silently until we know the name
                    acc.args.push_str(&arguments_delta);
                }
            }

            AgentResponseUpdate::ToolCalled { id, result, error } => {
                // Clean up accumulators — the tool has been executed and its
                // result reported.  Removing from the accumulator prevents stale
                // ToolCallingContent re-emission in finalize() when subsequent
                // loop iterations end with Finish(Stop).
                self.tool_accumulators.remove(&id);
                self.args_parsers.remove(&id);
                self.ended_calls.remove(&id);

                contents.push(Content::ToolCalled(ToolCalledContent {
                    meta: self.build_meta(),
                    call_id: id,
                    result,
                    error,
                }));
            }

            AgentResponseUpdate::Usage { usage } => {
                contents.push(Content::Usage(UsageContent {
                    meta: self.build_meta(),
                    usage,
                }));
            }
            AgentResponseUpdate::Finish {
                finish_reason: _,
                usage,
            } => {
                // Usage bundled with finish
                if let Some(u) = usage {
                    contents.push(Content::Usage(UsageContent {
                        meta: self.build_meta(),
                        usage: u,
                    }));
                }
                // The finish_reason is captured by the caller as pending.
                // Tool call flushing happens in finalize() based on finish_reason.
            }
            AgentResponseUpdate::Error { message } => {
                contents.push(Content::Error(ErrorContent {
                    meta: self.build_meta(),
                    error_code: "sse_parse_error".to_string(),
                    message,
                }));
            }
            AgentResponseUpdate::ResponseMetadata { id, model } => {
                if let Some(id) = id {
                    self.response_id = Some(id);
                }
                if let Some(model) = model {
                    self.response_model = Some(model.clone());
                    self.model_id = Some(model);
                }
            }
            AgentResponseUpdate::ToolApprovalRequest { name, arguments, .. } => {
                // arguments may be Value::String(json_str) — use as_str() to avoid
                // double serialization when displaying to the user.
                let args_display = arguments
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| serde_json::to_string(&arguments).unwrap_or_default());
                contents.push(Content::Text(TextContent {
                    meta: self.build_meta(),
                    delta: format!(
                        "\n[Approval required: {}({})]\n",
                        name,
                        args_display,
                    ),
                }));
            }
        }

        ConvertOutput { contents, events }
    }

    /// 生成带有 finish_reason 的最终 AgentResponseResult。
    /// 此处不发出 Usage——已在 consume() 流式传输期间发出。
    pub fn finalize(
        &mut self,
        finish_reason: Option<FinishReason>,
        _usage: Option<Usage>,
    ) -> AgentResponseResult {
        let mut contents = Vec::new();

        // Emit ToolCallEnd for any tool calls that were started but not yet ended
        // (covers cases where the stream ends without explicit End events).
        let call_ids: Vec<String> = self.tool_accumulators.keys().cloned().collect();
        let meta = self.build_meta();
        for call_id in &call_ids {
            if self.ended_calls.contains(call_id) {
                continue;
            }
            if let Some(acc) = self.tool_accumulators.get(call_id) {
                if acc.start_emitted && acc.name.is_some() {
                    contents.push(Content::ToolCallEnd(ToolCallEndContent {
                        meta: meta.clone(),
                        call_id: call_id.clone(),
                    }));
                }
            }
        }

        // Flush accumulated tool calls as complete ToolCallingContent.
        // Flush when the stream ended with ToolCalls (standard case), Stop
        // (covers FunctionInvokingChatClient which filters Finish(ToolCalls)
        // and replaces it with Finish(Stop) after tool execution), or
        // MaxRounds (tool loop forcibly terminated — accumulators are typically
        // empty here but this is defensive).
        if finish_reason == Some(FinishReason::ToolCalls)
            || finish_reason == Some(FinishReason::Stop)
            || finish_reason == Some(FinishReason::MaxRounds)
        {
            contents.extend(self.flush_tool_calls());
        }

        AgentResponseResult {
            id: self.response_id.take(),
            model: self.response_model.take(),
            finish_reason,
            contents,
            events: Vec::new(),
        }
    }

    /// Flush all accumulated tool call deltas as complete ToolCallingContent.
    /// This is the final emission when the stream ends with a ToolCalls finish_reason.
    fn flush_tool_calls(&mut self) -> Vec<Content> {
        let call_ids: Vec<String> = self.tool_accumulators.keys().cloned().collect();
        let mut result = Vec::new();
        let meta = self.build_meta();
        for call_id in call_ids {
            if let Some(acc) = self.tool_accumulators.remove(&call_id) {
                if acc.name.is_some() && !acc.args.is_empty() {
                    let arguments = serde_json::from_str(&acc.args)
                        .unwrap_or_else(|_| serde_json::Value::String(acc.args.clone()));
                    result.push(Content::ToolCalling(ToolCallingContent {
                        meta: meta.clone(),
                        call_id,
                        name: acc.name.expect("flush_tool_calls acc.name checked with is_some() above"),
                        arguments,
                    }));
                }
            }
        }
        result
    }
}

pub struct ConvertOutput {
    pub contents: Vec<Content>,
    pub events: Vec<Event>,
}

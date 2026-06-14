use std::collections::HashMap;

use chrono::Utc;
use rust_agent_core::{
    AgentId, AgentResponseResult, AgentResponseUpdate, AgentRunOptions, Content, ErrorContent,
    Event, FinishReason, ReasoningContent, ResponseMetadata, TextContent, ToolCallingContent,
    Usage, UsageContent,
};

/// Accumulator for streaming tool call deltas
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    id: Option<String>,
    name: Option<String>,
    args: String,
}

impl ToolCallAccumulator {
    fn is_complete(&self) -> bool {
        self.id.is_some() && self.name.is_some() && !self.args.is_empty()
    }

    fn to_tool_calling(&self, meta: &ResponseMetadata) -> ToolCallingContent {
        let arguments = serde_json::from_str(&self.args)
            .unwrap_or(serde_json::Value::String(self.args.clone()));
        ToolCallingContent {
            meta: meta.clone(),
            call_id: self.id.clone().unwrap(),
            name: self.name.clone().unwrap(),
            arguments,
        }
    }
}

/// Converts AgentResponseUpdate (internal SSE-level) to AgentResponseResult (public API)
pub struct AgentResponseConverter {
    agent_id: AgentId,
    model_id: Option<String>,
    executor_id: String,
    properties: HashMap<String, serde_json::Value>,
    // Internal state
    tool_accumulators: HashMap<usize, ToolCallAccumulator>,
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

    /// Consume a single AgentResponseUpdate, producing content and event vectors
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
            AgentResponseUpdate::ToolCallDelta {
                index,
                id,
                name,
                arguments_delta,
            } => {
                let is_complete = {
                    let acc = self.tool_accumulators.entry(index).or_default();
                    if let Some(id) = id {
                        acc.id = Some(id);
                    }
                    if let Some(name) = name {
                        acc.name = Some(name);
                    }
                    acc.args.push_str(&arguments_delta);
                    acc.is_complete()
                };

                // When the tool call delta is complete, emit ToolCallingContent
                if is_complete {
                    let acc = self.tool_accumulators.remove(&index).unwrap();
                    let meta = self.build_meta();
                    contents.push(Content::ToolCalling(acc.to_tool_calling(&meta)));
                }
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
                // Finish is handled via finalize(), but we may also emit
                // usage if it was bundled with finish
                if let Some(u) = usage {
                    contents.push(Content::Usage(UsageContent {
                        meta: self.build_meta(),
                        usage: u,
                    }));
                }
                // The finish_reason will be captured by the caller as pending
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
        }

        ConvertOutput { contents, events }
    }

    /// Produce the final AgentResponseResult with finish_reason.
    /// Usage is NOT emitted here — it is already emitted during streaming via consume().
    pub fn finalize(
        &mut self,
        finish_reason: Option<FinishReason>,
        _usage: Option<Usage>,
    ) -> AgentResponseResult {
        AgentResponseResult {
            id: self.response_id.take(),
            model: self.response_model.take(),
            finish_reason,
            contents: Vec::new(),
            events: Vec::new(),
        }
    }
}

pub struct ConvertOutput {
    pub contents: Vec<Content>,
    pub events: Vec<Event>,
}

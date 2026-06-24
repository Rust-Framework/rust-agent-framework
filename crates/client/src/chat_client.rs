use async_trait::async_trait;
use std::time::Duration;

use rust_agent_core::{
    AgentError, AgentResponseUpdate, BoxStream, ChatClientRunOptions, ChatMessage,
    IChatClient, MessageRole, ModelMetadata, Result,
};

use crate::options::ChatClientOptions;
use crate::transport::SseStream;
use crate::usage::UsageFormat;

/// 通用聊天客户端，处理 HTTP 传输和 SSE 流式层。
///
/// 适用于任何兼容 OpenAI 的 API（OpenAI、DeepSeek 等）。
/// 特定供应商的扩展通过包装类型（`OpenAiChatClient`、`DeepSeekChatClient`）实现，这些类型组合了该结构体。
pub struct ChatClient {
    http: reqwest::Client,
    options: ChatClientOptions,
}

impl ChatClient {
    pub fn new(options: ChatClientOptions) -> Result<Self> {
        let timeout = Duration::from_secs(options.timeout_secs.unwrap_or(60));
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| AgentError::ConfigError(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self { http, options })
    }

    pub fn options(&self) -> &ChatClientOptions {
        &self.options
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// 核心流式调用：向 `{api_base}/chat/completions` 发送 POST 请求，解析 SSE。
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
        usage_format: UsageFormat,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        let url = format!(
            "{}/chat/completions",
            self.options.api_base.trim_end_matches('/')
        );
        let body = self.build_request_body(messages, run_options);

        tracing::debug!(
            "ChatClient request to {} with model {}",
            url,
            self.options.model
        );

        let mut req = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.options.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        // Inject provider-specific extra headers
        for (key, value) in &self.options.extra_headers {
            req = req.header(key, value);
        }

        let response = req.send().await.map_err(|e| {
            AgentError::ChatClientError(format!("HTTP request failed: {}", e))
        })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AgentError::ChatClientError(format!(
                "Chat API returned {}: {}",
                status, error_text
            )));
        }

        let byte_stream = response.bytes_stream();
        let sse = SseStream::new(byte_stream, usage_format);
        Ok(Box::pin(sse))
    }

    /// 构建 OpenAI 兼容 `/chat/completions` 请求体。
    pub(crate) fn build_request_body(
        &self,
        messages: &[ChatMessage],
        run_options: &ChatClientRunOptions,
    ) -> serde_json::Value {
        let msgs: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                };

                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), serde_json::Value::String(role.into()));

                // OpenAI: assistant+tool_calls 时 content 应为 null 而非空字符串
                if m.role == MessageRole::Assistant
                    && m.tool_calls.is_some()
                    && m.content.is_empty()
                {
                    obj.insert("content".into(), serde_json::Value::Null);
                } else {
                    obj.insert("content".into(), serde_json::Value::String(m.content.clone()));
                }

                // name 仅用于 legacy function 消息；tool 角色不应携带 name
                if m.role != MessageRole::Tool {
                    if let Some(name) = &m.name {
                        obj.insert("name".into(), serde_json::Value::String(name.clone()));
                    }
                }
                // Serialize tool_calls for assistant messages
                if let Some(ref tool_calls) = m.tool_calls {
                    let tc_json: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            // tc.arguments may be Value::String(json_str) from
                            // function_invoking.rs (Path 1) or Value::Object(...) from
                            // converter.rs → session persistence (Path 2).
                            // .as_str() handles Path 1, .to_string() handles Path 2.
                            let args_str = match &tc.arguments {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": args_str,
                                }
                            })
                        })
                        .collect();
                    obj.insert("tool_calls".into(), serde_json::Value::Array(tc_json));
                }
                if let Some(ref tool_call_id) = m.tool_call_id {
                    obj.insert(
                        "tool_call_id".into(),
                        serde_json::Value::String(tool_call_id.clone()),
                    );
                }
                serde_json::Value::Object(obj)
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.options.model,
            "messages": msgs,
            "stream": true,
        });

        if self.options.stream_include_usage {
            body["stream_options"] = serde_json::json!({ "include_usage": true });
        }

        // Per-call overrides take precedence; fall back to client defaults
        let max_tokens = run_options.max_tokens.or(self.options.max_tokens);
        let temperature = run_options.temperature.or(self.options.temperature);
        let top_p = run_options.top_p.or(self.options.top_p);
        let stop = run_options.stop.as_ref().or(self.options.stop.as_ref());

        if let Some(mt) = max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(ref top_p) = top_p {
            body["top_p"] = serde_json::json!(top_p);
        }
        if let Some(ref stop) = stop {
            body["stop"] = serde_json::json!(stop);
        }

        // Merge per-call extra_body fields at top level
        if let Some(obj) = body.as_object_mut() {
            for (key, value) in &run_options.extra_body {
                obj.insert(key.clone(), value.clone());
            }
        }

        // Include tool definitions if provided
        if !run_options.tools.is_empty() {
            body["tools"] = serde_json::json!(run_options.tools);
        }

        // Include parallel_tool_calls setting if explicitly specified
        if let Some(parallel) = run_options.parallel_tool_calls {
            body["parallel_tool_calls"] = serde_json::json!(parallel);
        }

        body
    }
}

#[async_trait]
impl IChatClient for ChatClient {
    async fn run(
        &self,
        messages: &[ChatMessage],
        options: ChatClientRunOptions,
    ) -> Result<BoxStream<'static, Result<AgentResponseUpdate>>> {
        self.chat_stream(messages, &options, UsageFormat::OpenAI).await
    }

    fn model_id(&self) -> &str {
        &self.options.model
    }

    fn model_metadata(&self) -> Option<&ModelMetadata> {
        self.options.model_metadata.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_core::ToolCall;

    #[test]
    fn serializes_tool_loop_messages_openai_compatible() {
        let client = ChatClient::new(ChatClientOptions::openai("gpt-4", "sk-test")).unwrap();
        let messages = vec![
            ChatMessage::user("hi"),
            ChatMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                name: None,
                tool_calls: Some(vec![ToolCall {
                    id: "c1".into(),
                    name: "list_files".into(),
                    arguments: serde_json::json!({"path": "."}),
                }]),
                tool_call_id: None,
                source: None,
            },
            ChatMessage::tool("{\"ok\":true}", "c1"),
        ];
        let body = client.build_request_body(&messages, &ChatClientRunOptions::default());
        let msgs = body["messages"].as_array().unwrap();
        assert!(msgs[1]["content"].is_null());
        assert!(msgs[2].get("name").is_none());
        assert_eq!(msgs[2]["tool_call_id"], "c1");
    }
}
